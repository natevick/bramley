use std::path::Path;
use std::sync::Arc;

use fastframe_instance::Slot;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler, ServiceExt};
use serde_json::{Value, json};

use crate::cli;

pub const DISCLOSURE: &str = "bramley mcp: library data you request (search results, your playlists, what is playing) goes to the agent you connected. Playback starts only when that agent asks for it.";
const DISCLOSED: &str = "mcp-disclosed";
const SEARCH_TYPES: [&str; 4] = ["songs", "albums", "artists", "playlists"];
const ITEM_KEYS: [&str; 6] = [
    "id",
    "name",
    "artist",
    "album",
    "duration_ms",
    "track_count",
];

pub type Channel = Arc<dyn Fn(&[String]) -> Result<String, (i32, String)> + Send + Sync>;

struct Spec {
    name: &'static str,
    description: &'static str,
    args: &'static [(&'static str, &'static str)],
}

const ID: (&str, &str) = ("id", "string");
const TOOLS: &[Spec] = &[
    Spec {
        name: "search",
        description: "Search the Apple Music catalog. Returns up to 10 songs, albums, artists and playlists, each with the id that play and queue_add take. `types` narrows the kinds returned (songs, albums, artists, playlists). Needs Bramley running and signed in.",
        args: &[("term", "string"), ("types", "array")],
    },
    Spec {
        name: "play",
        description: "Play one catalog song now, replacing the queue. This starts audible playback: call it only when the user asked to hear something. Returns once Bramley has queued the command; call now_playing to see it take effect.",
        args: &[ID],
    },
    Spec {
        name: "pause",
        description: "Pause playback.",
        args: &[],
    },
    Spec {
        name: "resume",
        description: "Resume the current item. Refused if nothing has been played since Bramley started.",
        args: &[],
    },
    Spec {
        name: "next",
        description: "Skip to the next item in the queue. While paused this STARTS playback of the next item (MusicKit's behaviour), so call it only when the user wants to hear something.",
        args: &[],
    },
    Spec {
        name: "previous",
        description: "Go back to the previous item. Like next, this may start playback while paused.",
        args: &[],
    },
    Spec {
        name: "seek",
        description: "Jump to a position in the current item, in seconds from its start.",
        args: &[("seconds", "number")],
    },
    Spec {
        name: "queue_add",
        description: "Add a catalog song to the end of the queue. Does not start playback; refused if nothing has been played since Bramley started.",
        args: &[ID],
    },
    Spec {
        name: "now_playing",
        description: "What Bramley is playing: state, item, position and duration in seconds. `playing` is true only when the player has applied every command and audio is going out. If sign-in is needed or the audio output failed, `state` names that problem and `problem` says why, even when the player itself reports Playing.",
        args: &[],
    },
    Spec {
        name: "library_playlists",
        description: "The user's own Apple Music library playlists: the total count and the first 100 names and ids. This is the user's personal data; call it only when they ask about their library.",
        args: &[],
    },
];

fn schema(spec: &Spec) -> JsonObject {
    let mut properties = serde_json::Map::new();
    for (name, kind) in spec.args {
        let property = match *kind {
            "array" => json!({"type": "array", "items": {"type": "string", "enum": SEARCH_TYPES}}),
            kind => json!({ "type": kind }),
        };
        properties.insert((*name).into(), property);
    }
    let required = spec.args.iter().map(|(n, _)| *n);
    let required: Vec<&str> = required.filter(|n| *n != "types").collect();
    let schema = json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    });
    let Value::Object(schema) = schema else {
        unreachable!()
    };
    schema
}

pub fn tools() -> Vec<Tool> {
    let tool = |spec: &Spec| Tool::new(spec.name, spec.description, Arc::new(schema(spec)));
    TOOLS.iter().map(tool).collect()
}

fn text(args: &JsonObject, key: &str) -> Result<String, String> {
    match args.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        _ => Err(format!("`{key}` must be a non-empty string")),
    }
}

fn words(spec: &Spec, args: &JsonObject) -> Result<Vec<String>, String> {
    if let Some(key) = args.keys().find(|k| spec.args.iter().all(|(n, _)| n != k)) {
        return Err(format!("{} takes no argument `{key}`", spec.name));
    }
    let words = match spec.name {
        "search" => vec!["search".into(), text(args, "term")?],
        "play" => vec!["play".into(), text(args, "id")?],
        "queue_add" => vec!["queue".into(), "add".into(), text(args, "id")?],
        "seek" => match args.get("seconds").and_then(Value::as_f64) {
            Some(s) => vec!["seek".into(), s.to_string()],
            None => return Err("`seconds` must be a number".into()),
        },
        "resume" => vec!["play".into()],
        "previous" => vec!["prev".into()],
        "now_playing" => vec!["now".into()],
        "library_playlists" => vec!["library".into(), "playlists".into()],
        name => vec![name.into()],
    };
    cli::parse(&words).map_err(|e| e.to_string())?;
    Ok(words)
}

fn types(args: &JsonObject) -> Result<Vec<&str>, String> {
    let Some(asked) = args.get("types") else {
        return Ok(SEARCH_TYPES.to_vec());
    };
    let kinds = asked.as_array().filter(|a| !a.is_empty());
    let kinds = kinds.ok_or("`types` must be a non-empty list")?;
    kinds
        .iter()
        .map(|k| {
            let k = k.as_str().unwrap_or_default();
            SEARCH_TYPES.into_iter().find(|t| *t == k).ok_or_else(|| {
                format!("unknown type {k:?}; use songs, albums, artists or playlists")
            })
        })
        .collect()
}

fn item(item: &Value) -> Value {
    let kept = ITEM_KEYS.iter().filter(|k| !item[**k].is_null());
    Value::Object(kept.map(|k| ((*k).into(), item[*k].clone())).collect())
}

fn found(reply: &Value, kinds: &[&str]) -> Value {
    let kind = |k: &&str| {
        let items = reply["results"][*k]["items"].as_array().cloned();
        (
            (*k).into(),
            items.unwrap_or_default().iter().map(item).collect(),
        )
    };
    Value::Object(kinds.iter().map(kind).collect())
}

fn playlists(reply: &Value) -> Value {
    let page = &reply["results"];
    let items = page["items"].as_array().cloned().unwrap_or_default();
    json!({
        "total": page["total"],
        "returned": items.len(),
        "more": !page["next"].is_null(),
        "playlists": items.iter().map(|i| json!({"id": i["id"], "name": i["name"]})).collect::<Vec<_>>(),
    })
}

fn problem(banner: &str) -> (&'static str, String) {
    match banner {
        "SignInNeeded" => (
            "SignInNeeded",
            "sign in needed: run `bramley sign-in`".into(),
        ),
        b if b.starts_with("SigningIn(") => {
            let why = "a sign-in page is waiting in the browser";
            ("SigningIn", why.into())
        }
        b => {
            let inner = b.strip_prefix("Error(").and_then(|b| b.strip_suffix(')'));
            let inner = inner.unwrap_or(b);
            let why = serde_json::from_str::<String>(inner).unwrap_or_else(|_| inner.into());
            ("Error", why)
        }
    }
}

pub fn now_playing(status: &Value) -> Value {
    let player = status["state"].as_str().unwrap_or("Unknown");
    let problem = status["banner"].as_str().map(problem);
    let pending = status["applied"].as_u64() < status["seq"].as_u64();
    let playing = player == "Playing" && problem.is_none() && !pending;
    let state = problem.as_ref().map_or(player, |(state, _)| state);
    json!({
        "state": state,
        "playing": playing,
        "pending": pending,
        "playerState": player,
        "problem": problem.map(|(_, why)| why),
        "notice": status["notice"],
        "itemId": status["itemId"],
        "title": status["title"],
        "artist": status["artist"],
        "position": status["position"],
        "duration": status["duration"],
    })
}

pub fn call(channel: &Channel, name: &str, args: &JsonObject) -> Result<CallToolResult, ErrorData> {
    let Some(spec) = TOOLS.iter().find(|s| s.name == name) else {
        return Err(ErrorData::invalid_params(
            format!("unknown tool {name:?}"),
            None,
        ));
    };
    let refuse = |why: String| CallToolResult::error(vec![ContentBlock::text(why)]);
    let kinds = match (name, types(args)) {
        ("search", Err(why)) => return Ok(refuse(why)),
        (_, kinds) => kinds.unwrap_or_default(),
    };
    let words = match words(spec, args) {
        Ok(words) => words,
        Err(why) => return Ok(refuse(why)),
    };
    let reply = match channel(&words) {
        Ok(reply) => reply,
        Err((_, why)) => return Ok(refuse(why)),
    };
    if let Err((_, why)) = cli::outcome(&reply) {
        return Ok(refuse(why));
    }
    let reply: Value = serde_json::from_str(&reply).unwrap_or_default();
    Ok(CallToolResult::structured(match name {
        "search" => found(&reply, &kinds),
        "library_playlists" => playlists(&reply),
        _ => now_playing(&reply),
    }))
}

pub struct Server {
    channel: Channel,
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        let capabilities = ServerCapabilities::builder().enable_tools().build();
        ServerConfig::new(capabilities)
            .with_server_info(Implementation::new("bramley", env!("CARGO_PKG_VERSION")))
            .with_instructions(DISCLOSURE)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let channel = self.channel.clone();
        let args = request.arguments.unwrap_or_default();
        let ran = tokio::task::spawn_blocking(move || call(&channel, &request.name, &args)).await;
        let ran = ran.map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        ran.map(CallToolResponse::from)
    }
}

pub fn disclose(config: &Path, err: &mut impl std::io::Write) {
    let marker = config.join(DISCLOSED);
    if marker.exists() {
        return;
    }
    let _ = writeln!(err, "{DISCLOSURE}");
    let recorded = std::fs::create_dir_all(config)
        .and_then(|()| crate::credentials::write_private(&marker, "shown\n"));
    if let Err(e) = recorded {
        let _ = writeln!(
            err,
            "bramley mcp: not recorded ({e}); the notice will show again"
        );
    }
}

pub fn run(slot: Slot, config: &Path) -> anyhow::Result<()> {
    disclose(config, &mut std::io::stderr());
    let channel: Channel = Arc::new(move |args| cli::send(&slot, args));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let running = Server { channel }.serve(rmcp::transport::stdio()).await?;
        running.waiting().await?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    type Sent = Arc<Mutex<Vec<String>>>;

    fn fake(reply: Result<&'static str, (i32, &'static str)>) -> (Channel, Sent) {
        let sent: Sent = Arc::default();
        let seen = sent.clone();
        let channel: Channel = Arc::new(move |args: &[String]| {
            seen.lock().unwrap().push(args.join(" "));
            reply.map(str::to_owned).map_err(|(c, w)| (c, w.into()))
        });
        (channel, sent)
    }

    fn run(channel: &Channel, name: &str, args: &str) -> (bool, Value) {
        let args = serde_json::from_str(args).unwrap();
        let result = call(channel, name, &args).unwrap();
        let text = serde_json::to_value(&result.content).unwrap()[0]["text"].clone();
        (
            result.is_error == Some(true),
            result.structured_content.unwrap_or(text),
        )
    }

    const STATUS: &str = r#"{"state":"Playing","itemId":"6781024437","position":40.5,"applied":2,"seq":2,"banner":null,"tokenStorage":"keyring","signInUrl":"http://127.0.0.1:1/x"}"#;

    #[test]
    fn each_tool_sends_the_cli_words_for_its_command() {
        let (channel, sent) = fake(Ok(STATUS));
        let calls = [
            ("play", r#"{"id":"6781024437"}"#, "play 6781024437"),
            ("pause", "{}", "pause"),
            ("resume", "{}", "play"),
            ("next", "{}", "next"),
            ("previous", "{}", "prev"),
            ("seek", r#"{"seconds":12.5}"#, "seek 12.5"),
            ("queue_add", r#"{"id":"678107"}"#, "queue add 678107"),
            ("now_playing", "{}", "now"),
            ("library_playlists", "{}", "library playlists"),
            (
                "search",
                r#"{"term":"bohemian rhapsody"}"#,
                "search bohemian rhapsody",
            ),
        ];
        for (name, args, _) in calls {
            assert!(!run(&channel, name, args).0, "{name}");
        }
        let expected: Vec<&str> = calls.iter().map(|c| c.2).collect();
        assert_eq!(*sent.lock().unwrap(), expected);
        let (_, now) = run(&channel, "now_playing", "{}");
        assert!(now["itemId"] == "6781024437" && !now.to_string().contains("keyring"));
    }

    #[test]
    fn bad_arguments_are_tool_errors_and_never_reach_the_app() {
        let (channel, sent) = fake(Ok(STATUS));
        let bad = [
            ("play", "{}", "`id` must be a non-empty string"),
            ("play", r#"{"id":6781024437}"#, "`id` must be a non-empty"),
            ("play", r#"{"id":"x\");pwned(\""}"#, "is not a catalog id"),
            ("seek", r#"{"seconds":"10"}"#, "`seconds` must be a number"),
            ("seek", r#"{"seconds":-1}"#, "seek needs a number"),
            ("search", r#"{"term":""}"#, "`term` must be a non-empty"),
            (
                "search",
                r#"{"term":"q","types":"songs"}"#,
                "non-empty list",
            ),
            (
                "search",
                r#"{"term":"q","types":["videos"]}"#,
                "unknown type",
            ),
            ("pause", r#"{"now":true}"#, "pause takes no argument `now`"),
        ];
        for (name, args, why) in bad {
            let (failed, said) = run(&channel, name, args);
            assert!(
                failed && said.as_str().unwrap().contains(why),
                "{name} {args}: {said}"
            );
        }
        let unknown = call(&channel, "volume", &JsonObject::new()).unwrap_err();
        assert_eq!(unknown.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(sent.lock().unwrap().is_empty());
    }

    #[test]
    fn app_failures_become_tool_errors_with_the_apps_message() {
        let cases = [
            (
                Err((cli::NOT_RUNNING, "Bramley is not running")),
                "not running",
            ),
            (
                Ok(r#"{"refused":"sign in needed: nothing played"}"#),
                "sign in",
            ),
            (
                Ok(r#"{"error":"the command was not applied"}"#),
                "not applied",
            ),
        ];
        for (reply, why) in cases {
            let (channel, _) = fake(reply);
            for (name, args) in [
                ("play", r#"{"id":"1"}"#),
                ("now_playing", "{}"),
                ("library_playlists", "{}"),
            ] {
                let (failed, said) = run(&channel, name, args);
                assert!(
                    failed && said.as_str().unwrap().contains(why),
                    "{name}: {said}"
                );
            }
        }
    }

    #[test]
    fn now_playing_is_playing_only_with_no_banner_and_nothing_pending() {
        let cases = [
            ("{}", "Playing", true, None),
            (
                r#"{"banner":"Error(\"the audio output could not be opened\")"}"#,
                "Error",
                false,
                Some("the audio output could not be opened"),
            ),
            (
                r#"{"banner":"SignInNeeded"}"#,
                "SignInNeeded",
                false,
                Some("sign in needed: run `bramley sign-in`"),
            ),
            (
                r#"{"banner":"SigningIn(\"http://127.0.0.1:1/secret\")"}"#,
                "SigningIn",
                false,
                Some("a sign-in page is waiting in the browser"),
            ),
            (r#"{"seq":3}"#, "Playing", false, None),
            (r#"{"state":"Paused"}"#, "Paused", false, None),
        ];
        for (edit, state, playing, problem) in cases {
            let mut status: Value = serde_json::from_str(STATUS).unwrap();
            let edit: JsonObject = serde_json::from_str(edit).unwrap();
            status.as_object_mut().unwrap().extend(edit);
            let shown = now_playing(&status);
            assert_eq!(
                (shown["state"].as_str(), shown["playing"].as_bool()),
                (Some(state), Some(playing)),
                "{status}"
            );
            assert_eq!(
                (shown["problem"].as_str(), shown["playerState"].as_str()),
                (problem, status["state"].as_str())
            );
            let shown = shown.to_string();
            assert!(!shown.contains("127.0.0.1") && !shown.contains("keyring"));
        }
        let described = |name: &str| TOOLS.iter().find(|s| s.name == name).unwrap().description;
        assert!(described("next").contains("While paused this STARTS playback"));
        assert!(described("now_playing").contains("even when the player itself reports Playing"));
    }

    #[test]
    fn search_returns_only_the_asked_types_without_artwork() {
        let found = r#"{"results":{"songs":{"items":[{"id":"6781024437","kind":"songs","name":"Bohemian Rhapsody","artwork":"https://a/{w}x{h}.jpg","album":null}]},"albums":{"items":[]}}}"#;
        let (channel, _) = fake(Ok(found));
        let (_, all) = run(&channel, "search", r#"{"term":"queen"}"#);
        let song = json!({"id": "6781024437", "name": "Bohemian Rhapsody"});
        let none: Vec<Value> = Vec::new();
        assert_eq!(
            all,
            json!({"songs": [song], "albums": none, "artists": none, "playlists": none})
        );
        let (_, songs) = run(&channel, "search", r#"{"term":"queen","types":["songs"]}"#);
        assert_eq!(songs, json!({ "songs": [song] }));
    }

    #[test]
    fn library_playlists_reports_the_total_and_whether_more_pages_exist() {
        let page = r#"{"results":{"items":[{"id":"p.1","name":"Road"}],"next":"/v1/me/library/playlists?offset=100","total":140}}"#;
        let (channel, _) = fake(Ok(page));
        let (_, out) = run(&channel, "library_playlists", "{}");
        let one = json!([{"id": "p.1", "name": "Road"}]);
        assert_eq!(
            out,
            json!({"total": 140, "returned": 1, "more": true, "playlists": one})
        );
    }

    #[test]
    fn the_disclosure_is_shown_once_recorded_and_repeated_when_it_cannot_be() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
        let ok = dir.join(format!("mcp-ok-{}", std::process::id()));
        let file = ok.with_extension("file");
        let _ = std::fs::remove_dir_all(&ok);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, "a file, not a directory").unwrap();
        let shown = |config: &Path| {
            let mut err = Vec::new();
            disclose(config, &mut err);
            String::from_utf8(err).unwrap()
        };
        let runs = [shown(&ok), shown(&ok), shown(&file), shown(&file)];
        let recorded = ok.join(DISCLOSED).exists();
        std::fs::remove_dir_all(&ok).unwrap();
        std::fs::remove_file(&file).unwrap();
        assert!(
            recorded && runs[0] == format!("{DISCLOSURE}\n") && runs[1].is_empty(),
            "{runs:?}"
        );
        assert!(
            runs[2..]
                .iter()
                .all(|r| r.starts_with(DISCLOSURE) && r.contains("will show again"))
        );
        assert!(DISCLOSURE.contains("goes to the agent you connected"));
    }
}
