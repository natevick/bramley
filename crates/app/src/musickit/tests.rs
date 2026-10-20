use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bramley_engine::Tokens;

use super::{DENIED, NOT_AUTHORIZED, RESTRICTED, Session, TOKENS_UNAVAILABLE, UNANSWERED};

struct Fake {
    status: Mutex<&'static str>,
    answer: Mutex<&'static str>,
    fetches: AtomicUsize,
    developer: Mutex<&'static str>,
    broken: Mutex<bool>,
}

impl Fake {
    fn new(status: &'static str) -> Arc<Fake> {
        Arc::new(Fake {
            status: Mutex::new(status),
            answer: Mutex::new(status),
            fetches: AtomicUsize::new(0),
            developer: Mutex::new("dev-1"),
            broken: Mutex::new(false),
        })
    }

    fn session(self: &Arc<Fake>) -> Session {
        let (ask, get) = (self.clone(), self.clone());
        Session::new(
            move |request| {
                let answer = *ask.answer.lock().unwrap();
                Ok((if request {
                    answer
                } else {
                    *ask.status.lock().unwrap()
                })
                .into())
            },
            move |_| {
                get.fetches.fetch_add(1, Ordering::SeqCst);
                if *get.broken.lock().unwrap() {
                    return Err("MusicTokenRequestError: .developerTokenRequestFailed".into());
                }
                Ok(Tokens {
                    developer: (*get.developer.lock().unwrap()).into(),
                    user: "user-1".into(),
                })
            },
        )
    }

    fn fetched(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }
}

#[test]
fn only_an_authorized_status_counts_as_signed_in() {
    for (status, signed_in) in [
        ("authorized", true),
        ("notDetermined", false),
        ("denied", false),
        ("restricted", false),
        ("somethingNew", false),
    ] {
        let session = Fake::new(status).session();
        assert_eq!(session.authorized(), Ok(signed_in), "{status}");
    }
}

#[test]
fn a_sign_in_request_explains_every_answer_that_is_not_yes() {
    for (answer, outcome) in [
        ("authorized", Ok(true)),
        ("denied", Err(DENIED)),
        ("restricted", Err(RESTRICTED)),
        ("notDetermined", Err(UNANSWERED)),
    ] {
        let fake = Fake::new("notDetermined");
        *fake.answer.lock().unwrap() = answer;
        assert_eq!(fake.session().request(), outcome, "{answer}");
    }
    assert!(DENIED.contains("System Settings"));
}

#[test]
fn nothing_is_fetched_and_nobody_is_prompted_while_signed_out() {
    let fake = Fake::new("notDetermined");
    let session = fake.session();
    assert_eq!(session.developer(), Err(NOT_AUTHORIZED.to_owned()));
    assert_eq!(session.user(), Ok(None));
    assert_eq!((fake.fetched(), session.peek()), (0, None));
}

#[test]
fn signed_in_tokens_are_fetched_once_and_then_held() {
    let fake = Fake::new("authorized");
    let session = fake.session();
    assert_eq!(session.peek(), None);
    assert_eq!(session.developer(), Ok("dev-1".to_owned()));
    assert_eq!(session.user(), Ok(Some("user-1".to_owned())));
    assert_eq!(session.peek().as_deref(), Some("dev-1"));
    assert_eq!(fake.fetched(), 1);
    session.forget();
    assert_eq!(session.peek(), None);
}

#[test]
fn a_failed_token_fetch_is_recorded_and_the_next_success_clears_it() {
    let fake = Fake::new("authorized");
    let session = fake.session();
    *fake.broken.lock().unwrap() = true;
    assert_eq!(session.user(), Err(TOKENS_UNAVAILABLE));
    assert!(
        session
            .failure()
            .unwrap()
            .contains("developerTokenRequestFailed")
    );
    *fake.broken.lock().unwrap() = false;
    assert_eq!(session.user(), Ok(Some("user-1".to_owned())));
    assert_eq!(session.failure(), None);
}

#[test]
fn renewing_a_rejected_developer_token_refetches_once_a_minute() {
    let fake = Fake::new("authorized");
    let session = fake.session();
    assert_eq!(session.developer(), Ok("dev-1".to_owned()));
    *fake.developer.lock().unwrap() = "dev-2";
    assert!(session.renew("dev-1"));
    assert_eq!(
        (session.peek().as_deref(), fake.fetched()),
        (Some("dev-2"), 2)
    );
    assert!(session.renew("dev-1"));
    assert_eq!(fake.fetched(), 2);
    assert!(!session.renew("dev-2"));
    assert_eq!(fake.fetched(), 2);
}

#[test]
fn renewing_that_returns_the_same_token_reports_failure() {
    let fake = Fake::new("authorized");
    let session = fake.session();
    assert_eq!(session.developer(), Ok("dev-1".to_owned()));
    assert!(!session.renew("dev-1"));
    assert_eq!(fake.fetched(), 2);
}

#[test]
fn a_new_sign_in_drops_the_held_tokens() {
    let fake = Fake::new("authorized");
    let session = fake.session();
    session.developer().unwrap();
    session.request().unwrap();
    session.developer().unwrap();
    assert_eq!(fake.fetched(), 2);
}
