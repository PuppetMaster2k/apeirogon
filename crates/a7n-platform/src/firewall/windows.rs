// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads the firewall through the `INetFwPolicy2` COM API, which any user may
//! read.

use ::windows::Win32::Foundation::E_UNEXPECTED;
use ::windows::Win32::NetworkManagement::WindowsFirewall::{
    INetFwPolicy2, INetFwRule, NET_FW_ACTION_ALLOW, NET_FW_IP_PROTOCOL_ANY, NET_FW_IP_PROTOCOL_TCP,
    NET_FW_IP_PROTOCOL_UDP, NET_FW_PROFILE_TYPE2, NET_FW_RULE_DIR_IN, NetFwPolicy2,
};
use ::windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use ::windows::Win32::System::Ole::IEnumVARIANT;
use ::windows::Win32::System::Variant::{VARIANT, VT_DISPATCH, VariantClear};
use ::windows::core::{Interface, Result as ComResult};

use super::rules::{Action, Direction, Protocol, Rule, evaluate};
use super::{CheckError, Problem, Profile, Profiles};

/// Multicast DNS, which nearby discovery listens on. It is a well-known port
/// of the mDNS standard and not a protocol limit of Apeirogon.
const MDNS_PORT: u16 = 5353;

impl From<::windows::core::Error> for CheckError {
    fn from(error: ::windows::core::Error) -> Self {
        Self::Query(error.code().0)
    }
}

/// Keeps COM initialized on this thread; every COM object must drop first.
struct ComScope;

impl ComScope {
    fn enter() -> ComResult<Self> {
        // SAFETY: Called with no reserved pointer. Success, including
        // S_FALSE for an already initialized thread, is balanced by the
        // CoUninitialize in `drop`; failure returns before a scope exists.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
        Ok(Self)
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        // SAFETY: Balances the successful CoInitializeEx in `enter` on this thread.
        unsafe { CoUninitialize() };
    }
}

pub(super) fn check(port: u16) -> Result<Vec<Problem>, CheckError> {
    let program = std::env::current_exe()?;
    let _com = ComScope::enter()?;
    let policy = policy()?;
    let active = active_profiles(&policy)?;
    if active == Profiles::default() {
        return Ok(Vec::new());
    }
    let rules = read_rules(&policy)?;
    Ok(evaluate(&rules, active, &program, &[port, MDNS_PORT]))
}

fn policy() -> ComResult<INetFwPolicy2> {
    // SAFETY: The class id is the documented NetFwPolicy2 coclass, no
    // aggregation is requested, and COM is initialized by the caller.
    unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) }
}

/// Current profiles whose firewall is switched on.
fn active_profiles(policy: &INetFwPolicy2) -> ComResult<Profiles> {
    // SAFETY: `policy` is a live INetFwPolicy2 and the calls take no pointers.
    let current = unsafe { policy.CurrentProfileTypes() }?;
    let mut active = 0;
    for profile in Profile::ALL {
        let bit = profile.bit();
        if u32::try_from(current).unwrap_or_default() & bit == 0 {
            continue;
        }
        let type2 = NET_FW_PROFILE_TYPE2(i32::try_from(bit).unwrap_or_default());
        // SAFETY: As above.
        if unsafe { policy.get_FirewallEnabled(type2) }?.as_bool() {
            active |= bit;
        }
    }
    Ok(Profiles(active))
}

/// Rules that cannot be read are skipped, so one odd rule does not hide the rest.
fn read_rules(policy: &INetFwPolicy2) -> ComResult<Vec<Rule>> {
    // SAFETY: `policy` is a live INetFwPolicy2; the calls take no pointers.
    let rules = unsafe { policy.Rules() }?;
    // SAFETY: As above; `_NewEnum` returns an owned enumerator object.
    let enumerator: IEnumVARIANT = unsafe { rules._NewEnum() }?.cast()?;
    let mut read = Vec::new();
    loop {
        let mut item = [VARIANT::default()];
        let mut fetched = 0;
        // SAFETY: `item` holds one initialized (empty) VARIANT and `fetched`
        // outlives the call; the enumerator fills at most one element.
        unsafe { enumerator.Next(&mut item, &raw mut fetched) }.ok()?;
        if fetched == 0 {
            return Ok(read);
        }
        let [variant] = item;
        if let Ok(rule) = rule_of(variant).and_then(|rule| read_rule(&rule)) {
            read.push(rule);
        }
    }
}

/// Takes the rule out of the VARIANT the enumerator filled, then releases it.
fn rule_of(mut variant: VARIANT) -> ComResult<INetFwRule> {
    // SAFETY: The enumerator initialized `variant`; the union member is read
    // only after its type tag says it is an IDispatch pointer. The clone adds
    // its own reference, so clearing the VARIANT afterwards releases only the
    // enumerator's.
    let dispatch = unsafe {
        let inner = &variant.Anonymous.Anonymous;
        let dispatch = (inner.vt == VT_DISPATCH)
            .then(|| (*inner.Anonymous.pdispVal).clone())
            .flatten();
        VariantClear(&raw mut variant)?;
        dispatch
    };
    dispatch
        .ok_or_else(|| ::windows::core::Error::from(E_UNEXPECTED))?
        .cast()
}

fn read_rule(rule: &INetFwRule) -> ComResult<Rule> {
    // SAFETY: `rule` is a live INetFwRule and each getter returns an owned value.
    unsafe {
        let program = rule.ApplicationName()?.to_string();
        Ok(Rule {
            name: rule.Name()?.to_string(),
            enabled: rule.Enabled()?.as_bool(),
            direction: if rule.Direction()? == NET_FW_RULE_DIR_IN {
                Direction::Inbound
            } else {
                Direction::Outbound
            },
            action: if rule.Action()? == NET_FW_ACTION_ALLOW {
                Action::Allow
            } else {
                Action::Block
            },
            protocol: protocol(rule.Protocol()?),
            local_ports: rule.LocalPorts()?.to_string(),
            program: (!program.is_empty()).then(|| program.into()),
            profiles: Profiles(u32::try_from(rule.Profiles()?).unwrap_or_default()),
        })
    }
}

fn protocol(number: i32) -> Protocol {
    match number {
        n if n == NET_FW_IP_PROTOCOL_UDP.0 => Protocol::Udp,
        n if n == NET_FW_IP_PROTOCOL_TCP.0 => Protocol::Tcp,
        n if n == NET_FW_IP_PROTOCOL_ANY.0 => Protocol::Any,
        _ => Protocol::Other,
    }
}
