use super::*;
use pretty_assertions::assert_eq;

/// Quiescent guardians keep their original output and rebind to a recovered host without respawn.
#[tokio::test]
async fn closed_scope_output_survives_idle_and_host_recovery() -> TestResult {
    let fixture = Fixture::new()?;
    let mut child = fixture.launch("create")?;
    let original = ready(&fixture, &mut child).await?;
    let scope = ScopeId::new();
    let run = RunId::new();
    let mut client = fixture.client();
    client.execute(Operation::CreateScope { scope }).await?;
    let mut spec = RunSpec::new(
        "/bin/sh",
        fixture.directory.path(),
        DescendantPolicy::WaitForAll,
    );
    spec.args = ["-c", "printf retained-output"].map(Into::into).to_vec();
    spec.output = ora_process_protocol::OutputPolicy::Capture {
        stdout_limit: 64,
        stderr_limit: 0,
    };
    client
        .execute(Operation::Start {
            intent: HostRunIntent {
                scope,
                run,
                spec,
                host_disconnect: GuardianHostDisconnect::KeepRunning,
            },
        })
        .await?;
    observed(&client, run, |snapshot| {
        matches!(snapshot.cleanup, CleanupState::Complete(_))
    })
    .await?;
    client.execute(Operation::Close { scope }).await?;
    for recover in [false, true] {
        if recover {
            child.0.kill()?;
            child.0.wait()?;
            child = fixture.launch("recover")?;
            let current = ready(&fixture, &mut child).await?;
            assert_eq!(current.epoch.get(), original.epoch.get() + 1);
            client = fixture.client();
        }
        let deadline = Instant::now() + Duration::from_secs(/*secs*/ 10);
        loop {
            if let Reply::Scope(view) = client.execute(Operation::QueryScope { scope }).await?
                && view.last_observed
                    == Some(ScopeState::Closed(CleanupEvidence::BestEffortComplete))
                && view.coordination == ora_process_protocol::HostCoordination::Observing
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "original guardian was not rebound"
            );
            tokio::time::sleep(Duration::from_millis(/*millis*/ 5)).await;
        }
        assert_eq!(
            client
                .execute(Operation::Output {
                    run,
                    stream: ora_process_protocol::OutputStream::Stdout,
                    offset: 0,
                    max_bytes: 64,
                })
                .await?,
            Reply::Output {
                run,
                stream: ora_process_protocol::OutputStream::Stdout,
                offset: 0,
                output: ora_process_protocol::OutputRead {
                    bytes: b"retained-output".to_vec(),
                    retained: 15,
                    truncated: false,
                    state: ora_process_protocol::OutputState::Eof,
                },
            }
        );
    }
    Ok(())
}
