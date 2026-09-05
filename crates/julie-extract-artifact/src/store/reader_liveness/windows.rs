use sha2::{Digest, Sha256};
use winsafe::{HPROCESS, HPROCESSLIST, co};

use super::{ProcessIdentityObservation, ProcessIdentityUnknownReason, ProcessInstanceIdentity};

pub(super) fn inspect(pid: u32) -> ProcessIdentityObservation {
    let domain_digest = match current_domain_digest() {
        Ok(domain_digest) => domain_digest,
        Err(reason) => return ProcessIdentityObservation::Unknown(reason),
    };
    let process = match HPROCESS::OpenProcess(
        co::PROCESS::QUERY_LIMITED_INFORMATION | co::PROCESS::SYNCHRONIZE,
        false,
        pid,
    ) {
        Ok(process) => process,
        Err(error) if error == co::ERROR::ACCESS_DENIED => {
            return ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::AccessDenied);
        }
        Err(_) => {
            return match process_is_present(pid) {
                Ok(false) => ProcessIdentityObservation::Absent,
                Ok(true) => {
                    ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::ProbeFailed)
                }
                Err(reason) => ProcessIdentityObservation::Unknown(reason),
            };
        }
    };
    let (creation_time, _, _, _) = match process.GetProcessTimes() {
        Ok(times) => times,
        Err(error) => return ProcessIdentityObservation::Unknown(error_reason(error)),
    };
    let creation_identity = u64::from(creation_time);
    if creation_identity == 0 {
        return ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::ProbeFailed);
    }
    let identity = ProcessInstanceIdentity::new(
        pid,
        format!("windows:v1:{domain_digest}:{creation_identity:016x}"),
    );
    match process.WaitForSingleObject(Some(0)) {
        Ok(wait) if wait == co::WAIT::TIMEOUT => ProcessIdentityObservation::Alive(identity),
        Ok(wait) if wait == co::WAIT::OBJECT_0 => ProcessIdentityObservation::Terminated(identity),
        Ok(_) => ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::ProbeFailed),
        Err(error) => ProcessIdentityObservation::Unknown(error_reason(error)),
    }
}

pub(super) fn validate_identity_domain(
    original_birth_identity: &str,
) -> Result<(), ProcessIdentityUnknownReason> {
    let current_domain = current_domain_digest()?;
    validate_identity_domain_with_digest(original_birth_identity, &current_domain)
}

fn validate_identity_domain_with_digest(
    original_birth_identity: &str,
    current_domain: &str,
) -> Result<(), ProcessIdentityUnknownReason> {
    let original_domain = parse_windows_identity_domain(original_birth_identity)?;
    if original_domain == current_domain {
        Ok(())
    } else {
        Err(ProcessIdentityUnknownReason::IdentityDomainMismatch)
    }
}

fn current_domain_digest() -> Result<String, ProcessIdentityUnknownReason> {
    let machine_id =
        machine_uid::get().map_err(|_| ProcessIdentityUnknownReason::IdentityDomainUnverified)?;
    domain_digest(&machine_id).ok_or(ProcessIdentityUnknownReason::IdentityDomainUnverified)
}

fn domain_digest(machine_id: &str) -> Option<String> {
    let canonical = canonical_machine_guid(machine_id)?;
    let mut hasher = Sha256::new();
    hasher.update(b"julie-reader-domain:windows:v1\0");
    hasher.update(canonical.as_bytes());
    Some(format!("{:x}", hasher.finalize()))
}

fn canonical_machine_guid(machine_id: &str) -> Option<String> {
    let unbraced = machine_id
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .unwrap_or(machine_id);
    if unbraced.len() != 36 {
        return None;
    }
    for (index, byte) in unbraced.bytes().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return None;
            }
        } else if !byte.is_ascii_hexdigit() {
            return None;
        }
    }
    let canonical = unbraced.to_ascii_lowercase();
    if canonical.bytes().all(|byte| byte == b'0' || byte == b'-') {
        None
    } else {
        Some(canonical)
    }
}

fn parse_windows_identity_domain(
    birth_identity: &str,
) -> Result<&str, ProcessIdentityUnknownReason> {
    if !birth_identity.starts_with("windows:") {
        return Err(ProcessIdentityUnknownReason::IdentityDomainMismatch);
    }
    let fields = birth_identity.split(':').collect::<Vec<_>>();
    if fields.len() != 4
        || fields[0] != "windows"
        || fields[1] != "v1"
        || fields[2].len() != 64
        || !is_lower_hex(fields[2])
        || fields[2].bytes().all(|byte| byte == b'0')
        || fields[3].len() != 16
        || !is_lower_hex(fields[3])
        || u64::from_str_radix(fields[3], 16)
            .ok()
            .filter(|value| *value > 0)
            .is_none()
    {
        return Err(ProcessIdentityUnknownReason::InvalidBirthIdentity);
    }
    Ok(fields[2])
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn process_is_present(pid: u32) -> Result<bool, ProcessIdentityUnknownReason> {
    let mut snapshot = HPROCESSLIST::CreateToolhelp32Snapshot(co::TH32CS::SNAPPROCESS, None)
        .map_err(error_reason)?;
    for entry in snapshot.iter_processes() {
        let entry = entry.map_err(error_reason)?;
        if entry.th32ProcessID == pid {
            return Ok(true);
        }
    }
    Ok(false)
}

fn error_reason(error: co::ERROR) -> ProcessIdentityUnknownReason {
    if error == co::ERROR::ACCESS_DENIED {
        ProcessIdentityUnknownReason::AccessDenied
    } else {
        ProcessIdentityUnknownReason::ProbeFailed
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ProcessIdentityUnknownReason, canonical_machine_guid, domain_digest,
        parse_windows_identity_domain, validate_identity_domain_with_digest,
    };

    const MACHINE_ID: &str = "12340001-4980-1920-6788-123456789012";

    #[test]
    fn machine_guid_is_canonicalized_before_hashing() {
        assert_eq!(
            canonical_machine_guid("{12340001-4980-1920-6788-1234567890AB}"),
            Some("12340001-4980-1920-6788-1234567890ab".to_owned())
        );
        let digest = domain_digest(MACHINE_ID).unwrap();
        assert_eq!(digest.len(), 64);
        assert!(!digest.contains(MACHINE_ID));
    }

    #[test]
    fn malformed_and_zero_machine_guids_are_rejected() {
        for value in [
            "",
            "not-a-guid",
            "00000000-0000-0000-0000-000000000000",
            "12340001-4980-1920-6788-12345678901g",
        ] {
            assert_eq!(canonical_machine_guid(value), None);
        }
    }

    #[test]
    fn platform_and_malformed_windows_identities_are_rejected() {
        let digest = domain_digest(MACHINE_ID).unwrap();
        assert_eq!(
            parse_windows_identity_domain(
                "linux:v1:7f27acbd-5331-4b08-a56f-3c580f430912:4026531836:98765"
            ),
            Err(ProcessIdentityUnknownReason::IdentityDomainMismatch)
        );
        assert_eq!(
            parse_windows_identity_domain(&format!("windows:v1:{digest}:0")),
            Err(ProcessIdentityUnknownReason::InvalidBirthIdentity)
        );
        assert_eq!(
            parse_windows_identity_domain(
                "windows:v1:0000000000000000000000000000000000000000000000000000000000000000:0000000000000001"
            ),
            Err(ProcessIdentityUnknownReason::InvalidBirthIdentity)
        );
        assert_eq!(
            parse_windows_identity_domain(&format!("windows:v1:{digest}:0000000000000001")),
            Ok(digest.as_str())
        );
    }

    #[test]
    fn different_machine_domain_is_rejected() {
        let digest = domain_digest(MACHINE_ID).unwrap();
        let identity = format!("windows:v1:{digest}:0000000000000001");
        let other = domain_digest("22340001-4980-1920-6788-123456789012").unwrap();

        assert_eq!(
            validate_identity_domain_with_digest(&identity, &other),
            Err(ProcessIdentityUnknownReason::IdentityDomainMismatch)
        );
    }
}
