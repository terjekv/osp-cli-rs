use osp_cli::snapshot::{BackgroundRefresh, Snapshot, fresh, now};
use std::sync::{Arc, mpsc};
use std::time::Duration;

#[test]
fn snapshots_round_trip_through_the_product_writer_with_origin_and_freshness() {
    let dir = crate::temp_support::make_temp_dir("snapshot-round-trip");
    let path = dir.path().join("snapshot.json");
    let snapshot = Snapshot::new("https://inventory.example/team".into(), vec!["host-a"]);
    snapshot
        .save(Some(&path), |path, bytes| {
            std::fs::write(path, bytes)?;
            Ok(())
        })
        .unwrap();
    let loaded = Snapshot::<Vec<String>>::load(Some(&path), &snapshot.origin).unwrap();
    assert_eq!(loaded.snapshot, ["host-a"]);
    assert_eq!(loaded.fetched_at, snapshot.fetched_at);
    assert_eq!(loaded.attempted_at, snapshot.attempted_at);
    assert!(fresh(loaded.fetched_at));
    assert!(!fresh(now() - 3600));
    assert!(Snapshot::<Vec<String>>::load(Some(&path), "other-origin").is_none());
}

#[test]
fn refresh_coalesces_workers_and_retains_a_force_request_during_failure() {
    let refresh = Arc::new(BackgroundRefresh::default());
    let (started, runs) = mpsc::channel();
    let (release, barrier) = mpsc::channel();
    let barrier = std::sync::Mutex::new(barrier);
    refresh.schedule("snapshot-test", true, false, move |forced| {
        started.send(forced).unwrap();
        barrier
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        anyhow::bail!("fetch unavailable; caller keeps its published snapshot")
    });
    assert!(!runs.recv_timeout(Duration::from_secs(5)).unwrap());
    refresh.schedule("snapshot-test", true, true, |_| {
        panic!("second worker must coalesce")
    });
    release.send(()).unwrap();
    assert!(runs.recv_timeout(Duration::from_secs(5)).unwrap());
    release.send(()).unwrap();
}
