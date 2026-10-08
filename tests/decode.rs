use numera_cha_solver::{
    config::Settings,
    core::contracts::{Error, Input},
    engines::ocr::OCREngine,
    sdk::Numera,
};
use serde_json::json;
use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

#[tokio::test]
async fn cancelled_and_expired_decoders_hold_admission_until_drained() {
    for cancel in [true, false] {
        let root = tempfile::tempdir().expect("root");
        let mut settings = Settings::new(root.path().into());
        settings.queue_size = 1;
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let barrier = gate.clone();
        let (entered, mut started) = tokio::sync::mpsc::channel(1);
        let sdk = Numera::with_decoder(
            settings.clone(),
            OCREngine::new(settings),
            Arc::new(move |_, _| {
                let _ = entered.try_send(());
                let (lock, event) = &*barrier;
                let (_guard, wait) = event
                    .wait_timeout_while(
                        lock.lock().expect("gate"),
                        Duration::from_secs(3),
                        |ready| !*ready,
                    )
                    .expect("bounded decode");
                if wait.timed_out() {
                    return Err(Error::new("expired", "Controlled decoder ceiling."));
                }
                Err(Error::new(
                    "invalid_asset",
                    "Controlled decoder completion.",
                ))
            }),
        );
        let input =
            Input::from_value(json!({"kind":"image","asset":[1]})).expect("image observation");
        let run = sdk.clone();
        let deadline = tokio::time::Instant::now()
            + if cancel {
                Duration::from_secs(2)
            } else {
                Duration::from_millis(100)
            };
        let task = tokio::spawn(async move { run.recognize(input, Some(deadline)).await });
        tokio::time::timeout(Duration::from_secs(1), started.recv())
            .await
            .expect("decoder entered")
            .expect("event");
        if cancel {
            task.abort();
            assert!(task.await.expect_err("cancelled caller").is_cancelled());
        } else {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), task)
                    .await
                    .expect("deadline bounded")
                    .expect("joined")
                    .outcome,
                "expired"
            );
        }
        assert_eq!(
            sdk.recognize_value(json!({"kind":"text","text":"1+2"}), None)
                .await
                .error
                .expect("capacity retained")
                .code,
            "inference_busy"
        );
        {
            let (lock, event) = &*gate;
            *lock.lock().expect("release") = true;
            event.notify_all();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while !sdk.decode_idle() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("retained decoder finished");
        assert_eq!(
            sdk.recognize_value(json!({"kind":"text","text":"1+2"}), None)
                .await
                .answer
                .as_deref(),
            Some("3")
        );
        sdk.close().await;
        assert!(sdk.decode_idle());
    }
}

#[tokio::test]
async fn sdk_close_waits_for_owned_decoder_and_rejects_new_work() {
    let root = tempfile::tempdir().expect("root");
    let settings = Settings::new(root.path().into());
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let barrier = gate.clone();
    let (entered, mut started) = tokio::sync::mpsc::channel(1);
    let sdk = Numera::with_decoder(
        settings.clone(),
        OCREngine::new(settings),
        Arc::new(move |_, _| {
            let _ = entered.try_send(());
            let (lock, event) = &*barrier;
            let (_guard, _) = event
                .wait_timeout_while(lock.lock().expect("gate"), Duration::from_secs(3), |v| !*v)
                .expect("bounded decoder");
            Err(Error::new("invalid_asset", "Controlled completion."))
        }),
    );
    let run = sdk.clone();
    let task = tokio::spawn(async move {
        run.recognize_value(json!({"kind":"image","asset":[1]}), None)
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), started.recv())
        .await
        .expect("entered")
        .expect("event");
    task.abort();
    let _ = task.await;
    let owner = sdk.clone();
    let close = tokio::spawn(async move {
        owner.close().await;
    });
    tokio::task::yield_now().await;
    assert!(!close.is_finished());
    {
        let (lock, event) = &*gate;
        *lock.lock().expect("release") = true;
        event.notify_all();
    }
    tokio::time::timeout(Duration::from_secs(2), close)
        .await
        .expect("close bounded")
        .expect("joined");
    assert!(sdk.decode_idle());
    assert_eq!(
        sdk.recognize_value(json!({"kind":"text","text":"1+2"}), None)
            .await
            .outcome,
        "cancelled"
    );
}
