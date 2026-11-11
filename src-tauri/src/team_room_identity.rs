//! AI Team Room identity lifecycle primitives.
//!
//! This module is deliberately domain-only. It does not authenticate identities,
//! issue sessions, persist memberships, admit participants, or connect to an agent
//! runtime. A SessionSnapshot may only be populated by a future trusted auth adapter;
//! ExecutionEvidence must only be populated by a future verified runtime adapter.
//! Until those adapters exist, the desktop app must not expose these states as live
//! multi-agent presence.

pub const MAX_AI_PARTICIPANTS: usize = 15;
pub const DEFAULT_EXECUTION_HEARTBEAT_TTL_SECS: i64 = 45;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantRegistration {
    pub participant_id: String,
    pub principal_id: String,
    pub display_name: String,
    /// Registry metadata; never an authentication factor or permission grant.
    pub provider: String,
    /// UX role only; it carries no implicit system permissions.
    pub role: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub participant_id: String,
    pub principal_id: String,
    pub issued_at_unix: i64,
    pub expires_at_unix: i64,
    pub revoked_at_unix: Option<i64>,
}

impl SessionSnapshot {
    fn is_valid_for(&self, participant: &ParticipantRegistration, now_unix: i64) -> bool {
        self.participant_id == participant.participant_id
            && self.principal_id == participant.principal_id
            && !self.session_id.is_empty()
            && self.issued_at_unix <= now_unix
            && now_unix < self.expires_at_unix
            && self.revoked_at_unix.is_none()
    }
}

/// Evidence envelope reserved for a trusted runtime adapter.
/// Fields are private so UI/input payloads cannot manufacture execution evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionEvidence {
    session_id: String,
    participant_id: String,
    principal_id: String,
    worker_id: String,
    heartbeat_at_unix: i64,
    expires_at_unix: i64,
    revoked_at_unix: Option<i64>,
}

impl ExecutionEvidence {
    fn is_valid_for(
        &self,
        participant: &ParticipantRegistration,
        session: &SessionSnapshot,
        now_unix: i64,
        heartbeat_ttl_secs: i64,
    ) -> bool {
        self.session_id == session.session_id
            && self.participant_id == participant.participant_id
            && self.principal_id == participant.principal_id
            && !self.worker_id.is_empty()
            && self.heartbeat_at_unix <= now_unix
            && now_unix.saturating_sub(self.heartbeat_at_unix) <= heartbeat_ttl_secs
            && now_unix < self.expires_at_unix
            && self.revoked_at_unix.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticipantLifecycleState {
    /// Registered and enabled, but no authenticated session is present.
    Registered,
    /// A trusted auth adapter supplied a currently valid session snapshot.
    Connected,
    /// A trusted runtime adapter supplied fresh execution evidence for this session.
    Executing,
    /// A session snapshot exists but is expired, revoked, or bound to another identity.
    Disconnected,
    /// Registration has been disabled by an administrator.
    Disabled,
}

pub fn derive_lifecycle_state(
    participant: &ParticipantRegistration,
    session: Option<&SessionSnapshot>,
    execution: Option<&ExecutionEvidence>,
    now_unix: i64,
) -> ParticipantLifecycleState {
    if !participant.enabled {
        return ParticipantLifecycleState::Disabled;
    }

    let Some(session) = session else {
        return ParticipantLifecycleState::Registered;
    };

    if !session.is_valid_for(participant, now_unix) {
        return ParticipantLifecycleState::Disconnected;
    }

    if execution.is_some_and(|evidence| {
        evidence.is_valid_for(
            participant,
            session,
            now_unix,
            DEFAULT_EXECUTION_HEARTBEAT_TTL_SECS,
        )
    }) {
        return ParticipantLifecycleState::Executing;
    }

    ParticipantLifecycleState::Connected
}

/// Pure capacity predicate for domain validation only.
/// This is NOT a concurrency-safe admission mechanism; production admission must
/// serialize the room row and membership/use-count changes in a shared transaction.
pub fn capacity_available(current_active_ai_participants: usize) -> bool {
    current_active_ai_participants < MAX_AI_PARTICIPANTS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn participant() -> ParticipantRegistration {
        ParticipantRegistration {
            participant_id: "participant-1".into(),
            principal_id: "workload:provider:subject-1".into(),
            display_name: "Research AI".into(),
            provider: "provider-registry-id".into(),
            role: "RESEARCH".into(),
            enabled: true,
        }
    }

    fn session() -> SessionSnapshot {
        SessionSnapshot {
            session_id: "session-1".into(),
            participant_id: "participant-1".into(),
            principal_id: "workload:provider:subject-1".into(),
            issued_at_unix: 100,
            expires_at_unix: 200,
            revoked_at_unix: None,
        }
    }

    fn execution() -> ExecutionEvidence {
        ExecutionEvidence {
            session_id: "session-1".into(),
            participant_id: "participant-1".into(),
            principal_id: "workload:provider:subject-1".into(),
            worker_id: "runtime-worker-1".into(),
            heartbeat_at_unix: 150,
            expires_at_unix: 190,
            revoked_at_unix: None,
        }
    }

    #[test]
    fn registration_alone_never_means_connected_or_executing() {
        assert_eq!(
            derive_lifecycle_state(&participant(), None, None, 150),
            ParticipantLifecycleState::Registered
        );
    }

    #[test]
    fn valid_session_means_connected_but_not_executing() {
        assert_eq!(
            derive_lifecycle_state(&participant(), Some(&session()), None, 150),
            ParticipantLifecycleState::Connected
        );
    }

    #[test]
    fn fresh_session_bound_runtime_evidence_means_executing() {
        assert_eq!(
            derive_lifecycle_state(
                &participant(),
                Some(&session()),
                Some(&execution()),
                160
            ),
            ParticipantLifecycleState::Executing
        );
    }

    #[test]
    fn mismatched_or_expired_session_is_disconnected() {
        let mut mismatched = session();
        mismatched.principal_id = "workload:other:subject".into();
        assert_eq!(
            derive_lifecycle_state(&participant(), Some(&mismatched), None, 150),
            ParticipantLifecycleState::Disconnected
        );

        assert_eq!(
            derive_lifecycle_state(&participant(), Some(&session()), None, 200),
            ParticipantLifecycleState::Disconnected
        );
    }

    #[test]
    fn stale_or_cross_session_execution_evidence_does_not_mark_executing() {
        let mut stale = execution();
        stale.heartbeat_at_unix = 100;
        assert_eq!(
            derive_lifecycle_state(
                &participant(),
                Some(&session()),
                Some(&stale),
                160
            ),
            ParticipantLifecycleState::Connected
        );

        let mut wrong_session = execution();
        wrong_session.session_id = "another-session".into();
        assert_eq!(
            derive_lifecycle_state(
                &participant(),
                Some(&session()),
                Some(&wrong_session),
                160
            ),
            ParticipantLifecycleState::Connected
        );
    }

    #[test]
    fn disabled_registration_is_never_connected() {
        let mut disabled = participant();
        disabled.enabled = false;
        assert_eq!(
            derive_lifecycle_state(&disabled, Some(&session()), Some(&execution()), 160),
            ParticipantLifecycleState::Disabled
        );
    }

    #[test]
    fn capacity_predicate_accepts_fourteenth_and_fifteenth_but_not_sixteenth() {
        assert!(capacity_available(13));
        assert!(capacity_available(14));
        assert!(!capacity_available(15));
    }
}
