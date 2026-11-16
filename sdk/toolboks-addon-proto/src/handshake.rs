//! The `Hello` exchange. The host speaks first and chooses the version (the one recorded for
//! the installed add-on); the add-on accepts any version it speaks, from
//! [`MIN_PROTOCOL_VERSION`] up to its own, and answers with the host's. Each side refuses
//! anything else with a plain message before it returns [`ProtocolError::VersionMismatch`].

use crate::{
    AddonMessage, FrameReader, FrameWriter, Hello, HostMessage, ProtocolError, MIN_PROTOCOL_VERSION,
};
use std::io::{Read, Write};

/// Host side: sends `hello`, waits for the add-on's `Hello` and checks version and id. The
/// add-on must answer with exactly the version in `hello`, which is then the connection's.
pub fn host_handshake<R: Read, W: Write>(
    reader: &mut FrameReader<R>,
    writer: &mut FrameWriter<W>,
    hello: &Hello,
) -> Result<Hello, ProtocolError> {
    writer.write(&HostMessage::Hello(hello.clone()))?;
    match reader.read::<AddonMessage>()? {
        AddonMessage::Hello(theirs) => {
            let error = if theirs.protocol_version != hello.protocol_version {
                ProtocolError::VersionMismatch {
                    addon: theirs.protocol_version,
                    host: hello.protocol_version,
                }
            } else if theirs.addon_id != hello.addon_id {
                ProtocolError::WrongAddon {
                    expected: hello.addon_id.clone(),
                    actual: theirs.addon_id,
                }
            } else {
                return Ok(theirs);
            };
            // Best effort: the add-on may already be gone.
            let _ = writer.write(&HostMessage::Refuse {
                protocol_version: hello.protocol_version,
                reason: error.to_string(),
            });
            Err(error)
        }
        AddonMessage::Refuse {
            protocol_version,
            reason,
        } => Err(if protocol_version != hello.protocol_version {
            ProtocolError::VersionMismatch {
                addon: protocol_version,
                host: hello.protocol_version,
            }
        } else {
            ProtocolError::Refused(reason)
        }),
        _ => Err(ProtocolError::Unexpected("add-on message before Hello")),
    }
}

/// Add-on side: waits for the host's `Hello`. A host version from [`MIN_PROTOCOL_VERSION`] up
/// to `hello.protocol_version` (the newest the add-on speaks) is accepted: the add-on answers
/// with `hello` at the host's version and returns that version, the connection's. Any other
/// version is refused in plain words.
pub fn addon_handshake<R: Read, W: Write>(
    reader: &mut FrameReader<R>,
    writer: &mut FrameWriter<W>,
    hello: &Hello,
) -> Result<u32, ProtocolError> {
    match reader.read::<HostMessage>()? {
        HostMessage::Hello(theirs) => {
            let version = theirs.protocol_version;
            if !(MIN_PROTOCOL_VERSION..=hello.protocol_version).contains(&version) {
                let error = ProtocolError::VersionMismatch {
                    addon: hello.protocol_version,
                    host: version,
                };
                let _ = writer.write(&AddonMessage::Refuse {
                    protocol_version: hello.protocol_version,
                    reason: error.to_string(),
                });
                return Err(error);
            }
            writer.write(&AddonMessage::Hello(Hello {
                protocol_version: version,
                ..hello.clone()
            }))?;
            Ok(version)
        }
        HostMessage::Refuse { reason, .. } => Err(ProtocolError::Refused(reason)),
        _ => Err(ProtocolError::Unexpected("host message before Hello")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::duplex;

    #[test]
    fn host_refuses_newer_addon_in_plain_words() {
        let (host, addon) = duplex();
        let fake = std::thread::spawn(move || {
            let (mut reader, mut writer) = addon.split();
            let _ = reader.read::<HostMessage>().unwrap();
            let mut hello = Hello::new("probe", "9.9.9");
            hello.protocol_version = 99;
            writer.write(&AddonMessage::Hello(hello)).unwrap();
            reader.read::<HostMessage>().unwrap()
        });
        let (mut reader, mut writer) = host.split();
        let error =
            host_handshake(&mut reader, &mut writer, &Hello::new("probe", "0.13.0")).unwrap_err();
        let text = format!(
            "Add-on speaks protocol 99, Toolboks speaks {}. Update the add-on or Toolboks.",
            crate::PROTOCOL_VERSION
        );
        assert_eq!(error.to_string(), text);
        match fake.join().unwrap() {
            HostMessage::Refuse { reason, .. } => assert_eq!(reason, text),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// Runs `addon_handshake` with the add-on's `ours` against a host that sends `host_hello`.
    /// Returns the add-on's result and what the host read back.
    fn addon_meets(ours: Hello, host_hello: Hello) -> (Result<u32, ProtocolError>, AddonMessage) {
        let (host, addon) = duplex();
        let side = std::thread::spawn(move || {
            let (mut reader, mut writer) = addon.split();
            addon_handshake(&mut reader, &mut writer, &ours)
        });
        let (mut reader, mut writer) = host.split();
        writer.write(&HostMessage::Hello(host_hello)).unwrap();
        let answer = reader.read::<AddonMessage>().unwrap();
        (side.join().unwrap(), answer)
    }

    fn text(addon: u32, host: u32) -> String {
        format!(
            "Add-on speaks protocol {addon}, Toolboks speaks {host}. Update the add-on or Toolboks."
        )
    }

    #[test]
    fn addon_agrees_on_every_version_it_speaks_and_answers_with_it() {
        for version in MIN_PROTOCOL_VERSION..=crate::PROTOCOL_VERSION {
            let (agreed, answer) = addon_meets(
                Hello::new("probe", "0.5.0"),
                Hello::with_version("probe", "0.14.0", version),
            );
            assert_eq!(agreed.unwrap(), version);
            assert_eq!(
                answer,
                AddonMessage::Hello(Hello {
                    protocol_version: version,
                    addon_id: "probe".into(),
                    software_version: "0.5.0".into(),
                })
            );
        }
    }

    #[test]
    fn addon_refuses_a_newer_or_older_host_in_plain_words() {
        let newest = crate::PROTOCOL_VERSION;
        for host in [newest + 1, 6, 99, MIN_PROTOCOL_VERSION - 1, 0] {
            let (agreed, answer) = addon_meets(
                Hello::new("probe", "0.5.0"),
                Hello::with_version("probe", "0.14.0", host),
            );
            let error = agreed.unwrap_err();
            assert!(matches!(error, ProtocolError::VersionMismatch { .. }));
            assert_eq!(error.to_string(), text(newest, host));
            assert_eq!(
                answer,
                AddonMessage::Refuse {
                    protocol_version: newest,
                    reason: text(newest, host),
                }
            );
        }
    }

    #[test]
    fn an_addon_that_speaks_only_3_refuses_a_host_at_4() {
        // What a protocol 3 exe does: its newest and only version is 3.
        let (agreed, answer) = addon_meets(
            Hello::with_version("probe", "0.4.0", 3),
            Hello::with_version("probe", "0.14.0", 4),
        );
        assert_eq!(agreed.unwrap_err().to_string(), text(3, 4));
        assert!(matches!(
            answer,
            AddonMessage::Refuse {
                protocol_version: 3,
                ..
            }
        ));
        // And it accepts a host at 3.
        let (agreed, _) = addon_meets(
            Hello::with_version("probe", "0.4.0", 3),
            Hello::with_version("probe", "0.14.0", 3),
        );
        assert_eq!(agreed.unwrap(), 3);
    }

    #[test]
    fn host_at_3_and_addon_at_4_agree_on_3() {
        let (host, addon) = duplex();
        let side = std::thread::spawn(move || {
            let (mut reader, mut writer) = addon.split();
            addon_handshake(&mut reader, &mut writer, &Hello::new("probe", "0.5.0"))
        });
        let (mut reader, mut writer) = host.split();
        let theirs = host_handshake(
            &mut reader,
            &mut writer,
            &Hello::with_version("probe", "0.14.0", 3),
        )
        .unwrap();
        assert_eq!(theirs.protocol_version, 3);
        assert_eq!(side.join().unwrap().unwrap(), 3);
    }

    #[test]
    fn host_refuses_an_answer_at_another_version_than_it_sent() {
        // A peer that answers 5 to a host at 4, or 3 to a host at 4, is refused.
        for (sent, answered) in [(4, 5), (4, 3), (3, 4)] {
            let (host, addon) = duplex();
            let fake = std::thread::spawn(move || {
                let (mut reader, mut writer) = addon.split();
                let _ = reader.read::<HostMessage>().unwrap();
                writer
                    .write(&AddonMessage::Hello(Hello::with_version(
                        "probe", "9.9.9", answered,
                    )))
                    .unwrap();
                reader.read::<HostMessage>().unwrap()
            });
            let (mut reader, mut writer) = host.split();
            let error = host_handshake(
                &mut reader,
                &mut writer,
                &Hello::with_version("probe", "0.14.0", sent),
            )
            .unwrap_err();
            assert_eq!(error.to_string(), text(answered, sent));
            match fake.join().unwrap() {
                HostMessage::Refuse {
                    protocol_version,
                    reason,
                } => {
                    assert_eq!(protocol_version, sent);
                    assert_eq!(reason, text(answered, sent));
                }
                other => panic!("expected a refusal, got {other:?}"),
            }
        }
    }

    #[test]
    fn host_reports_an_addon_refusal_at_another_version_as_a_mismatch() {
        // A protocol 5 add-on refuses a host at 4 with its own version in the refusal.
        let (host, addon) = duplex();
        std::thread::spawn(move || {
            let (mut reader, mut writer) = addon.split();
            let _ = reader.read::<HostMessage>().unwrap();
            writer
                .write(&AddonMessage::Refuse {
                    protocol_version: 5,
                    reason: text(5, 4),
                })
                .unwrap();
        });
        let (mut reader, mut writer) = host.split();
        let error = host_handshake(
            &mut reader,
            &mut writer,
            &Hello::with_version("probe", "0.14.0", 4),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), text(5, 4));
    }

    #[test]
    fn host_refuses_wrong_addon_id() {
        let (host, addon) = duplex();
        let fake = std::thread::spawn(move || {
            let (mut reader, mut writer) = addon.split();
            addon_handshake(&mut reader, &mut writer, &Hello::new("other", "0.4.0")).unwrap();
        });
        let (mut reader, mut writer) = host.split();
        let error =
            host_handshake(&mut reader, &mut writer, &Hello::new("probe", "0.13.0")).unwrap_err();
        assert!(matches!(error, ProtocolError::WrongAddon { .. }));
        fake.join().unwrap();
    }
}
