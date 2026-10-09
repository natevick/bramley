use super::in_child;
use crate::cdm::Widevine;

#[cfg(target_os = "linux")]
#[test]
fn retrying_a_library_without_cdm_exports_fails_cleanly_every_time() {
    let test = "tests::cdm::retrying_a_library_without_cdm_exports_fails_cleanly_every_time";
    let child = in_child(test, &[], || {
        for attempt in 0..3 {
            let err = Widevine::create("libm.so.6".as_ref())
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
    let mut cdm = Widevine::create(&crate::EngineConfig::default_cdm_path()).unwrap();
    let kid = [
        0x67, 0xb3, 0x0c, 0x86, 0x75, 0x6f, 0x57, 0xc5, 0xa0, 0xa3, 0x8a, 0x23, 0xac, 0x8c, 0x91,
        0x78,
    ];
    let system_id = [
        0xed, 0xef, 0x8b, 0xa9, 0x79, 0xd6, 0x4a, 0xce, 0xa3, 0xc8, 0x27, 0xdc, 0xd5, 0x1d, 0x21,
        0xed,
    ];
    let data = [&[0x12u8, 0x10][..], &kid].concat();
    let body = [
        &[0u8; 4][..],
        &system_id,
        &(data.len() as u32).to_be_bytes(),
        &data,
    ]
    .concat();
    let pssh = [&((body.len() + 8) as u32).to_be_bytes()[..], b"pssh", &body].concat();
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
