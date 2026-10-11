//! Plumb session. X1.4 provides the transcript-as-source session turn pipeline around a
//! caller-supplied active compiler stage (Hotfix 060): transcript evidence, bounded context,
//! inference request preparation, persisted-acquisition verification, output validation and
//! X1.3 intake of every proposal. It never calls a provider or writes the graph.

mod model;
mod turn;

pub use model::{
    InspectedSessionOutput, Session, SessionContextItem, SessionContextSource, SessionError,
    SessionInferenceContext, SessionIntakeInputs, SessionModelOutput, SessionProposalCard,
    SessionRole, SessionTurnConfig, SessionTurnPlan, SessionTurnResult, SessionUserTurn,
    TranscriptTurn, SESSION_CONTEXT_VERSION, SESSION_MAX_PROPOSALS, SESSION_OUTPUT_VERSION,
    SESSION_TURN_TASK_KIND, TRANSCRIPT_MEDIA_TYPE, TRANSCRIPT_SOURCE_KIND, TRANSCRIPT_VERSION,
};
pub use turn::{complete_turn, inspect_turn_output, prepare_turn};
