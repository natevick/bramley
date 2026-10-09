#include <dlfcn.h>

#include <chrono>
#include <cstdio>
#include <cstring>
#include <deque>
#include <functional>
#include <map>
#include <memory>
#include <string>
#include <vector>

#include "../cdm-api/content_decryption_module.h"

extern "C" {
struct ShimCallbacks {
  void* ctx;
  void (*on_initialized)(void* ctx, bool success);
  void (*on_new_session)(void* ctx, uint32_t promise_id, const char* sid, uint32_t sid_len);
  void (*on_resolve)(void* ctx, uint32_t promise_id);
  void (*on_reject)(void* ctx, uint32_t promise_id, uint32_t exception, uint32_t system_code, const char* msg, uint32_t msg_len);
  void (*on_message)(void* ctx, const char* sid, uint32_t sid_len, uint32_t type, const char* msg, uint32_t msg_len);
  void (*on_key)(void* ctx, const char* sid, uint32_t sid_len, const uint8_t* kid, uint32_t kid_len, uint32_t status, uint32_t system_code);
  void (*on_keys_done)(void* ctx, const char* sid, uint32_t sid_len, bool has_additional_usable_key);
  void (*on_expiration)(void* ctx, const char* sid, uint32_t sid_len, double expiry);
  void (*on_closed)(void* ctx, const char* sid, uint32_t sid_len);
  void (*on_log)(void* ctx, const char* msg);
};
}

namespace {

using InitFn = void (*)();
using CreateFn = void* (*)(int, const char*, uint32_t, GetCdmHostFunc, void*);
using VersionFn = const char* (*)();
using DeinitFn = void (*)();

struct Library {
  void* handle = nullptr;
  InitFn init = nullptr;
  CreateFn create = nullptr;
  VersionFn version = nullptr;
  DeinitFn deinit = nullptr;
};

Library g_lib;

class BufferImpl final : public cdm::Buffer {
 public:
  explicit BufferImpl(uint32_t capacity) : data_(capacity), size_(0) {}
  void Destroy() override { delete this; }
  uint32_t Capacity() const override { return static_cast<uint32_t>(data_.size()); }
  uint8_t* Data() override { return data_.data(); }
  void SetSize(uint32_t size) override { size_ = size; }
  uint32_t Size() const override { return size_; }

 private:
  ~BufferImpl() override = default;
  std::vector<uint8_t> data_;
  uint32_t size_;
};

class DecryptedBlockImpl final : public cdm::DecryptedBlock {
 public:
  DecryptedBlockImpl() = default;
  ~DecryptedBlockImpl() override {
    if (buffer_) buffer_->Destroy();
  }
  void SetDecryptedBuffer(cdm::Buffer* buffer) override {
    if (buffer_) buffer_->Destroy();
    buffer_ = buffer;
  }
  cdm::Buffer* DecryptedBuffer() override { return buffer_; }
  void SetTimestamp(int64_t timestamp) override { timestamp_ = timestamp; }
  int64_t Timestamp() const override { return timestamp_; }

 private:
  cdm::Buffer* buffer_ = nullptr;
  int64_t timestamp_ = 0;
};

class Host;

class MemoryFileIO final : public cdm::FileIO {
 public:
  MemoryFileIO(Host* host, cdm::FileIOClient* client) : host_(host), client_(client) {}
  ~MemoryFileIO() override = default;
  void Open(const char* file_name, uint32_t file_name_size) override;
  void Read() override;
  void Write(const uint8_t* data, uint32_t data_size) override;
  void Close() override;
  bool closed = false;

 private:
  Host* host_;
  cdm::FileIOClient* client_;
  std::string name_;
};

class Host final : public cdm::Host_10 {
 public:
  explicit Host(const ShimCallbacks& callbacks) : cb_(callbacks) {}
  ~Host() override = default;

  cdm::ContentDecryptionModule_10* cdm = nullptr;
  std::map<std::string, std::vector<uint8_t>> files;
  std::deque<std::function<void()>> tasks;
  std::vector<std::unique_ptr<MemoryFileIO>> file_ios;

  void Log(const std::string& msg) {
    if (cb_.on_log) cb_.on_log(cb_.ctx, msg.c_str());
  }

  int64_t NextTimerMs() {
    if (!tasks.empty()) return 0;
    if (timers_.empty()) return -1;
    auto now = std::chrono::steady_clock::now();
    auto best = timers_.front().first;
    for (auto& t : timers_) best = std::min(best, t.first);
    auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(best - now).count();
    return ms < 0 ? 0 : ms;
  }

  void Pump() {
    auto now = std::chrono::steady_clock::now();
    std::vector<void*> due;
    for (auto it = timers_.begin(); it != timers_.end();) {
      if (it->first <= now) {
        due.push_back(it->second);
        it = timers_.erase(it);
      } else {
        ++it;
      }
    }
    for (void* context : due) cdm->TimerExpired(context);
    while (!tasks.empty()) {
      auto task = std::move(tasks.front());
      tasks.pop_front();
      task();
    }
  }

  cdm::Buffer* Allocate(uint32_t capacity) override { return new BufferImpl(capacity); }

  void SetTimer(int64_t delay_ms, void* context) override {
    timers_.emplace_back(std::chrono::steady_clock::now() + std::chrono::milliseconds(delay_ms), context);
  }

  cdm::Time GetCurrentWallTime() override {
    auto now = std::chrono::system_clock::now().time_since_epoch();
    return std::chrono::duration<double>(now).count();
  }

  void OnInitialized(bool success) override { cb_.on_initialized(cb_.ctx, success); }

  void OnResolveKeyStatusPromise(uint32_t promise_id, cdm::KeyStatus key_status) override {
    Log("OnResolveKeyStatusPromise status=" + std::to_string(key_status));
    cb_.on_resolve(cb_.ctx, promise_id);
  }

  void OnResolveNewSessionPromise(uint32_t promise_id, const char* session_id, uint32_t session_id_size) override {
    cb_.on_new_session(cb_.ctx, promise_id, session_id, session_id_size);
  }

  void OnResolvePromise(uint32_t promise_id) override { cb_.on_resolve(cb_.ctx, promise_id); }

  void OnRejectPromise(uint32_t promise_id, cdm::Exception exception, uint32_t system_code, const char* error_message, uint32_t error_message_size) override {
    cb_.on_reject(cb_.ctx, promise_id, exception, system_code, error_message, error_message_size);
  }

  void OnSessionMessage(const char* session_id, uint32_t session_id_size, cdm::MessageType message_type, const char* message, uint32_t message_size) override {
    cb_.on_message(cb_.ctx, session_id, session_id_size, message_type, message, message_size);
  }

  void OnSessionKeysChange(const char* session_id, uint32_t session_id_size, bool has_additional_usable_key, const cdm::KeyInformation* keys_info, uint32_t keys_info_count) override {
    for (uint32_t i = 0; i < keys_info_count; ++i) {
      cb_.on_key(cb_.ctx, session_id, session_id_size, keys_info[i].key_id, keys_info[i].key_id_size, keys_info[i].status, keys_info[i].system_code);
    }
    cb_.on_keys_done(cb_.ctx, session_id, session_id_size, has_additional_usable_key);
  }

  void OnExpirationChange(const char* session_id, uint32_t session_id_size, cdm::Time new_expiry_time) override {
    cb_.on_expiration(cb_.ctx, session_id, session_id_size, new_expiry_time);
  }

  void OnSessionClosed(const char* session_id, uint32_t session_id_size) override { cb_.on_closed(cb_.ctx, session_id, session_id_size); }

  void SendPlatformChallenge(const char*, uint32_t, const char*, uint32_t) override {
    Log("SendPlatformChallenge requested; answering unavailable");
    tasks.push_back([this] {
      cdm::PlatformChallengeResponse empty{};
      cdm->OnPlatformChallengeResponse(empty);
    });
  }

  void EnableOutputProtection(uint32_t desired_protection_mask) override {
    Log("EnableOutputProtection mask=" + std::to_string(desired_protection_mask));
  }

  void QueryOutputProtectionStatus() override {
    Log("QueryOutputProtectionStatus requested; answering no display link");
    tasks.push_back([this] { cdm->OnQueryOutputProtectionStatus(cdm::kQuerySucceeded, cdm::kLinkTypeNone, cdm::kProtectionNone); });
  }

  void OnDeferredInitializationDone(cdm::StreamType stream_type, cdm::Status decoder_status) override {
    Log("OnDeferredInitializationDone " + std::to_string(stream_type) + " " + std::to_string(decoder_status));
  }

  cdm::FileIO* CreateFileIO(cdm::FileIOClient* client) override {
    Log("CreateFileIO (in-memory, never written to disk)");
    file_ios.push_back(std::make_unique<MemoryFileIO>(this, client));
    return file_ios.back().get();
  }

  void RequestStorageId(uint32_t version) override {
    Log("RequestStorageId version=" + std::to_string(version) + "; answering unavailable");
    tasks.push_back([this, version] { cdm->OnStorageId(version, nullptr, 0); });
  }

 private:
  ShimCallbacks cb_;
  std::vector<std::pair<std::chrono::steady_clock::time_point, void*>> timers_;
};

void MemoryFileIO::Open(const char* file_name, uint32_t file_name_size) {
  name_.assign(file_name, file_name_size);
  host_->Log("FileIO::Open " + name_);
  host_->tasks.push_back([this] {
    if (!closed) client_->OnOpenComplete(cdm::FileIOClient::Status::kSuccess);
  });
}

void MemoryFileIO::Read() {
  host_->Log("FileIO::Read " + name_);
  host_->tasks.push_back([this] {
    if (closed) return;
    auto& data = host_->files[name_];
    client_->OnReadComplete(cdm::FileIOClient::Status::kSuccess, data.data(), static_cast<uint32_t>(data.size()));
  });
}

void MemoryFileIO::Write(const uint8_t* data, uint32_t data_size) {
  host_->Log("FileIO::Write " + name_ + " bytes=" + std::to_string(data_size));
  host_->files[name_].assign(data, data + data_size);
  host_->tasks.push_back([this] {
    if (!closed) client_->OnWriteComplete(cdm::FileIOClient::Status::kSuccess);
  });
}

void MemoryFileIO::Close() {
  host_->Log("FileIO::Close " + name_);
  closed = true;
  Host* host = host_;
  host->tasks.push_back([host, this] {
    auto& ios = host->file_ios;
    for (auto it = ios.begin(); it != ios.end(); ++it) {
      if (it->get() == this) {
        ios.erase(it);
        return;
      }
    }
  });
}

int g_requested_host_version = -1;

void* GetHost(int host_interface_version, void* user_data) {
  g_requested_host_version = host_interface_version;
  auto* host = static_cast<Host*>(user_data);
  if (host_interface_version == cdm::Host_10::kVersion) return static_cast<cdm::Host_10*>(host);
  return nullptr;
}

}

extern "C" {

int shim_load(const char* path, char* err, uint32_t err_cap) {
  if (g_lib.handle) return 0;
  g_lib.handle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
  if (!g_lib.handle) {
    std::snprintf(err, err_cap, "%s", dlerror());
    return -1;
  }
  g_lib.init = reinterpret_cast<InitFn>(dlsym(g_lib.handle, "InitializeCdmModule_4"));
  g_lib.create = reinterpret_cast<CreateFn>(dlsym(g_lib.handle, "CreateCdmInstance"));
  g_lib.version = reinterpret_cast<VersionFn>(dlsym(g_lib.handle, "GetCdmVersion"));
  g_lib.deinit = reinterpret_cast<DeinitFn>(dlsym(g_lib.handle, "DeinitializeCdmModule"));
  if (!g_lib.init || !g_lib.create || !g_lib.version) {
    std::snprintf(err, err_cap, "missing entry points");
    dlclose(g_lib.handle);
    g_lib = Library{};
    return -2;
  }
  g_lib.init();
  return 0;
}

const char* shim_cdm_version() { return g_lib.version ? g_lib.version() : ""; }

int shim_requested_host_version() { return g_requested_host_version; }

void* shim_create(int interface_version, const ShimCallbacks* callbacks) {
  if (!g_lib.create) return nullptr;
  auto* host = new Host(*callbacks);
  static const char kKeySystem[] = "com.widevine.alpha";
  void* instance = g_lib.create(interface_version, kKeySystem, sizeof(kKeySystem) - 1, &GetHost, host);
  if (!instance) {
    delete host;
    return nullptr;
  }
  host->cdm = static_cast<cdm::ContentDecryptionModule_10*>(instance);
  return host;
}

void shim_initialize(void* handle) {
  auto* host = static_cast<Host*>(handle);
  host->cdm->Initialize(false, false, false);
}

void shim_set_server_certificate(void* handle, uint32_t promise_id, const uint8_t* data, uint32_t size) {
  static_cast<Host*>(handle)->cdm->SetServerCertificate(promise_id, data, size);
}

void shim_create_session(void* handle, uint32_t promise_id, uint32_t init_data_type, const uint8_t* init_data, uint32_t init_data_size) {
  auto* host = static_cast<Host*>(handle);
  host->cdm->CreateSessionAndGenerateRequest(promise_id, cdm::kTemporary, static_cast<cdm::InitDataType>(init_data_type), init_data, init_data_size);
}

void shim_update_session(void* handle, uint32_t promise_id, const char* session_id, uint32_t session_id_size, const uint8_t* response, uint32_t response_size) {
  static_cast<Host*>(handle)->cdm->UpdateSession(promise_id, session_id, session_id_size, response, response_size);
}

void shim_close_session(void* handle, uint32_t promise_id, const char* session_id, uint32_t session_id_size) {
  static_cast<Host*>(handle)->cdm->CloseSession(promise_id, session_id, session_id_size);
}

void shim_remove_session(void* handle, uint32_t promise_id, const char* session_id, uint32_t session_id_size) {
  static_cast<Host*>(handle)->cdm->RemoveSession(promise_id, session_id, session_id_size);
}

uint32_t shim_decrypt(void* handle, uint32_t scheme, uint32_t crypt_blocks, uint32_t skip_blocks, const uint8_t* data, uint32_t data_size, const uint8_t* key_id, uint32_t key_id_size, const uint8_t* iv, uint32_t iv_size, const uint32_t* subsample_pairs, uint32_t num_subsamples, int64_t timestamp, uint8_t* out, uint32_t out_capacity, uint32_t* out_size) {
  auto* host = static_cast<Host*>(handle);
  std::vector<cdm::SubsampleEntry> subsamples(num_subsamples);
  for (uint32_t i = 0; i < num_subsamples; ++i) {
    subsamples[i].clear_bytes = subsample_pairs[2 * i];
    subsamples[i].cipher_bytes = subsample_pairs[2 * i + 1];
  }
  cdm::InputBuffer_2 input{};
  input.data = data;
  input.data_size = data_size;
  input.encryption_scheme = scheme == 2 ? cdm::EncryptionScheme::kCbcs : cdm::EncryptionScheme::kCenc;
  input.key_id = key_id;
  input.key_id_size = key_id_size;
  input.iv = iv;
  input.iv_size = iv_size;
  input.subsamples = subsamples.empty() ? nullptr : subsamples.data();
  input.num_subsamples = num_subsamples;
  input.pattern = {crypt_blocks, skip_blocks};
  input.timestamp = timestamp;
  DecryptedBlockImpl block;
  cdm::Status status = host->cdm->Decrypt(input, &block);
  *out_size = 0;
  if (status == cdm::kSuccess && block.DecryptedBuffer()) {
    uint32_t size = block.DecryptedBuffer()->Size();
    if (size > out_capacity) return 1000;
    std::memcpy(out, block.DecryptedBuffer()->Data(), size);
    *out_size = size;
  }
  return status;
}

void shim_pump(void* handle) { static_cast<Host*>(handle)->Pump(); }

int64_t shim_next_timer_ms(void* handle) { return static_cast<Host*>(handle)->NextTimerMs(); }

void shim_destroy(void* handle) {
  auto* host = static_cast<Host*>(handle);
  host->cdm->Destroy();
  delete host;
}

}
