use super::in_child;
use crate::cdm::Widevine;

#[cfg(target_os = "linux")]
#[test]
fn retrying_a_library_without_cdm_exports_fails_cleanly_every_time() {
    let test = "tests::cdm::retrying_a_library_without_cdm_exports_fails_cleanly_every_time";
    let child = in_child(test, &[], || {
        for attempt in 0..3 {
            let err = Widevine::create("libm.so.6".as_ref(), false)
                .err()
                .map(|e| e.to_string());
            assert!(
                err.as_deref()
                    .is_some_and(|e| e.contains("missing entry points")),
                "attempt {attempt}: {err:?}"
            );
        }
    });
    if let Some(c) = child {
        assert!(c.status.success(), "{:?}\n{}", c.status, c.stderr);
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "needs the Chrome Widevine CDM installed"]
fn real_cdm_initializes_and_answers_a_license_request_on_the_creating_thread() {
    use crate::cdm::CdmEvent;
    let mut cdm = Widevine::create(&crate::EngineConfig::default_cdm_path(), false).unwrap();
    let pssh: Vec<u8> = "000000327073736800000000edef8ba979d64acea3c827dcd51d21ed000000121210\
        67b30c86756f57c5a0a38a23ac8c9178"
        .as_bytes()
        .chunks(2)
        .map(|h| u8::from_str_radix(std::str::from_utf8(h).unwrap(), 16).unwrap())
        .collect();
    let mut events = Vec::new();
    let pump = |cdm: &mut Box<dyn crate::cdm::CdmBackend>, events: &mut Vec<CdmEvent>| {
        for _ in 0..100 {
            events.extend(cdm.poll());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    pump(&mut cdm, &mut events);
    cdm.create_session(1, 0, &pssh);
    pump(&mut cdm, &mut events);
    assert_eq!(events.first(), Some(&CdmEvent::Initialized(true)));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, CdmEvent::NewSession { promise: 1, .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, CdmEvent::Message { kind: 0, body, .. } if body.len() > 100))
    );
    drop(cdm);
}
