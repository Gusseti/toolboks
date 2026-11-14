//! The `Hello` exchange. The host speaks first; each side checks the other's version and
//! refuses a mismatch with a plain message before it returns
//! [`ProtocolError::VersionMismatch`].

use crate::{AddonMessage, FrameReader, FrameWriter, Hello, HostMessage, ProtocolError};
use std::io::{Read, Write};

/// Host side: sends `hello`, waits for the add-on's `Hello` and checks version and id.
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

/// Add-on side: waits for the host's `Hello`, refuses a version mismatch, then answers
/// with `hello`. Returns the host's `Hello`.
pub fn addon_handshake<R: Read, W: Write>(
    reader: &mut FrameReader<R>,
    writer: &mut FrameWriter<W>,
    hello: &Hello,
) -> Result<Hello, ProtocolError> {
    match reader.read::<HostMessage>()? {
        HostMessage::Hello(theirs) => {
            if theirs.protocol_version != hello.protocol_version {
                let error = ProtocolError::VersionMismatch {
                    addon: hello.protocol_version,
                    host: theirs.protocol_version,
                };
                let _ = writer.write(&AddonMessage::Refuse {
                    protocol_version: hello.protocol_version,
                    reason: error.to_string(),
                });
                return Err(error);
            }
            writer.write(&AddonMessage::Hello(hello.clone()))?;
            Ok(theirs)
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
