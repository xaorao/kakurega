//! The hardware check: is a model's signing key bound into a quote from genuine Intel TDX hardware?
//!
//! NEAR's stateless verifier contract (`near/mpc`, `crates/tee-verifier`) checks a TDX quote against Intel's collateral with
//! the block time and returns the verified report. NEAR AI Cloud puts a model's Ed25519 signing key in the first half of the
//! quote's report data and the caller's challenge in the second half. This module reads that report and decides what the
//! registry may record. The types mirror `tee-verifier-interface` (MIT), field for field, so the returned bytes decode.

use near_sdk::json_types::U64;
use near_sdk::{AccountId, env, near};

/// What a challenge must be prefixed with before hashing, so a quote made for another purpose is never accepted here.
pub const CHALLENGE_PREFIX: &str = "kakurega-attest-v1";

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum VerificationResultIn {
    Verified(VerifiedReportIn),
    Rejected(VerifierErrorIn),
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub enum VerifierErrorIn {
    DcapVerification(String),
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct VerifiedReportIn {
    pub status: String,
    pub advisory_ids: Vec<String>,
    pub report: ReportIn,
    pub ppid: Vec<u8>,
    pub qe_status: TcbStatusWithAdvisoryIn,
    pub platform_status: TcbStatusWithAdvisoryIn,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ReportIn {
    SgxEnclave(EnclaveReportIn),
    TD10(TDReport10In),
    TD15(TDReport15In),
    TD15Ex(TDReport15ExIn),
}

impl ReportIn {
    /// The TD 1.0 body every TDX report carries; `None` for an SGX enclave report.
    pub fn as_td10(&self) -> Option<&TDReport10In> {
        match self {
            ReportIn::TD10(r) => Some(r),
            ReportIn::TD15(r) => Some(&r.base),
            ReportIn::TD15Ex(r) => Some(&r.base.base),
            ReportIn::SgxEnclave(_) => None,
        }
    }
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct TDReport10In {
    pub tee_tcb_svn: [u8; 16],
    pub mr_seam: [u8; 48],
    pub mr_signer_seam: [u8; 48],
    pub seam_attributes: [u8; 8],
    pub td_attributes: [u8; 8],
    pub xfam: [u8; 8],
    pub mr_td: [u8; 48],
    pub mr_config_id: [u8; 48],
    pub mr_owner: [u8; 48],
    pub mr_owner_config: [u8; 48],
    pub rt_mr0: [u8; 48],
    pub rt_mr1: [u8; 48],
    pub rt_mr2: [u8; 48],
    pub rt_mr3: [u8; 48],
    pub report_data: [u8; 64],
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct TDReport15In {
    pub base: TDReport10In,
    pub tee_tcb_svn2: [u8; 16],
    pub mr_service_td: [u8; 48],
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct TDReport15ExIn {
    pub base: TDReport15In,
    pub vmid: u8,
    pub td_id: [u8; 32],
    pub dev_info: [u8; 48],
    pub init_service_td_hash: [u8; 48],
    pub init_service_td_attributes: [u8; 8],
    pub init_cpu_svn: [u8; 16],
    pub init_tee_tcb_svn: [u8; 16],
    pub init_tee_fmspc: [u8; 12],
    pub cur_service_td_hash: [u8; 48],
    pub cur_service_td_attributes: [u8; 8],
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct EnclaveReportIn {
    pub cpu_svn: [u8; 16],
    pub misc_select: u32,
    pub reserved1: [u8; 28],
    pub attributes: [u8; 16],
    pub mr_enclave: [u8; 32],
    pub reserved2: [u8; 32],
    pub mr_signer: [u8; 32],
    pub reserved3: [u8; 96],
    pub isv_prod_id: u16,
    pub isv_svn: u16,
    pub reserved4: [u8; 60],
    pub report_data: [u8; 64],
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub enum TcbStatusIn {
    UpToDate,
    OutOfDateConfigurationNeeded,
    OutOfDate,
    ConfigurationAndSWHardeningNeeded,
    ConfigurationNeeded,
    SWHardeningNeeded,
    Revoked,
    TDRelaunchAdvised,
    TDRelaunchAdvisedConfigurationNeeded,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct TcbStatusWithAdvisoryIn {
    pub status: TcbStatusIn,
    pub advisory_ids: Vec<String>,
}

/// What the registry records about a signing key whose quote the verifier accepted. Lowercase hex throughout.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attested {
    /// The Ed25519 key bound into the quote: 64 hex digits.
    pub signer: String,
    /// Intel's verdict on the platform's patch level, as the verifier reported it (for example `UpToDate`).
    pub tcb_status: String,
    pub advisory_ids: Vec<String>,
    /// The measurement of the virtual machine's initial contents.
    pub mr_td: String,
    pub rt_mr0: String,
    pub rt_mr1: String,
    pub rt_mr2: String,
    pub rt_mr3: String,
    /// SHA-256 of the deployment's compose description, when the quote carries one in the form NEAR AI Cloud uses.
    pub compose_sha256: Option<String>,
    /// The account whose challenge the quote answered, and that paid for this record.
    pub attested_by: AccountId,
    /// Block time at which the verifier checked the quote, in nanoseconds since 1970.
    pub attested_at: U64,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The 32 bytes a quote must carry as its challenge to be recorded for `account`.
pub fn challenge(account: &AccountId) -> [u8; 32] {
    env::sha256_array(format!("{CHALLENGE_PREFIX}:{account}").as_bytes())
}

/// Reads a verified report. `Err` carries the reason in words; nothing is recorded then.
pub fn accept(
    v: &VerifiedReportIn,
    account: &AccountId,
    now: u64,
) -> Result<Attested, &'static str> {
    let Some(td) = v.report.as_td10() else {
        return Err("not a TDX report");
    };
    if td.report_data[32..] != challenge(account) {
        return Err("the quote does not answer this account's challenge");
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&td.report_data[..32]);
    if crate::is_small_order(&key) {
        return Err("the bound key is not a usable signing key");
    }
    if v.status.is_empty() || v.status.len() > 64 || v.advisory_ids.len() > 64 {
        return Err("the verifier's status is not in a recordable form");
    }
    if v.advisory_ids.iter().any(|a| a.len() > 64) {
        return Err("the verifier's status is not in a recordable form");
    }
    let compose_sha256 = (td.mr_config_id[0] == 0x01).then(|| hex(&td.mr_config_id[1..33]));
    Ok(Attested {
        signer: hex(&key),
        tcb_status: v.status.clone(),
        advisory_ids: v.advisory_ids.clone(),
        mr_td: hex(&td.mr_td),
        rt_mr0: hex(&td.rt_mr0),
        rt_mr1: hex(&td.rt_mr1),
        rt_mr2: hex(&td.rt_mr2),
        rt_mr3: hex(&td.rt_mr3),
        compose_sha256,
        attested_by: account.clone(),
        attested_at: U64(now),
    })
}

/// The verifier's argument bytes: the quote as a Borsh byte vector, then the collateral exactly as given (already Borsh).
pub fn verifier_args(quote: &[u8], collateral: &[u8]) -> Vec<u8> {
    let mut args = Vec::with_capacity(4 + quote.len() + collateral.len());
    args.extend((quote.len() as u32).to_le_bytes());
    args.extend(quote);
    args.extend(collateral);
    args
}
