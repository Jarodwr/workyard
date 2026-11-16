//! Validated domain values. String and JSON parsing belongs at adapter/storage boundaries.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ops::Deref;

use crate::{Error, Result};

macro_rules! entity_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl TryFrom<&str> for $name {
            type Error = Error;
            fn try_from(value: &str) -> Result<Self> {
                if value.is_empty() {
                    Err(Error::new(
                        "validation_error",
                        concat!(stringify!($name), " cannot be empty"),
                    ))
                } else {
                    Ok(Self(value.to_owned()))
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

entity_id!(BoardId);
entity_id!(CardId);
entity_id!(QuestionId);
entity_id!(LaneId);
entity_id!(PhaseId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneKind {
    Backlog,
    Active,
    Done,
    // Kept for reading and cleaning up databases created before lanes were fixed.
    Review,
    Custom,
}

impl LaneKind {
    pub const WORK: [Self; 3] = [Self::Backlog, Self::Active, Self::Done];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Active => "active",
            Self::Done => "done",
            Self::Review => "review",
            Self::Custom => "custom",
        }
    }
    pub fn is_work(self) -> bool {
        Self::WORK.contains(&self)
    }
}

impl TryFrom<&str> for LaneKind {
    type Error = Error;
    fn try_from(value: &str) -> Result<Self> {
        match value {
            "backlog" => Ok(Self::Backlog),
            "active" => Ok(Self::Active),
            "done" => Ok(Self::Done),
            "review" => Ok(Self::Review),
            "custom" => Ok(Self::Custom),
            _ => Err(Error::new(
                "database_error",
                format!("unknown lane kind: {value}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionKind {
    Engineering,
    Business,
}

impl TryFrom<&str> for QuestionKind {
    type Error = Error;
    fn try_from(value: &str) -> Result<Self> {
        match value {
            "engineering" => Ok(Self::Engineering),
            "business" => Ok(Self::Business),
            _ => Err(Error::new(
                "validation_error",
                "kind must be engineering or business",
            )),
        }
    }
}

impl QuestionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Engineering => "engineering",
            Self::Business => "business",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionStatus {
    Open,
    Answered,
    Dropped,
}

impl QuestionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Answered => "answered",
            Self::Dropped => "dropped",
        }
    }
}

impl TryFrom<&str> for QuestionStatus {
    type Error = Error;
    fn try_from(value: &str) -> Result<Self> {
        match value {
            "open" => Ok(Self::Open),
            "answered" => Ok(Self::Answered),
            "dropped" => Ok(Self::Dropped),
            _ => Err(Error::new(
                "database_error",
                format!("unknown question status: {value}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
}

impl TryFrom<&str> for Priority {
    type Error = Error;
    fn try_from(value: &str) -> Result<Self> {
        match value {
            "P0" => Ok(Self::P0),
            "P1" => Ok(Self::P1),
            "P2" => Ok(Self::P2),
            "P3" => Ok(Self::P3),
            _ => Err(Error::new(
                "validation_error",
                "priority must be P0, P1, P2, or P3",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct Progress(i64);
impl TryFrom<i64> for Progress {
    type Error = Error;
    fn try_from(value: i64) -> Result<Self> {
        if (0..=100).contains(&value) {
            Ok(Self(value))
        } else {
            Err(Error::new(
                "validation_error",
                "progress must be between 0 and 100",
            ))
        }
    }
}
impl From<Progress> for i64 {
    fn from(value: Progress) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct Version(i64);
impl TryFrom<i64> for Version {
    type Error = Error;
    fn try_from(value: i64) -> Result<Self> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err(Error::new("validation_error", "version must be positive"))
        }
    }
}
impl From<Version> for i64 {
    fn from(value: Version) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DependencyTarget {
    Card { id: CardId },
    Question { id: QuestionId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blocker {
    #[serde(rename = "type")]
    pub kind: BlockerKind,
    pub id: String,
    pub key: String,
    pub title: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewGate {
    pub change_id: String,
    pub working_branch: String,
    pub commit_oid: String,
    pub approved: bool,
    pub reviewer: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockerKind {
    Card,
    Question,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: CardId,
    pub key: String,
    pub board_id: BoardId,
    pub lane_id: LaneId,
    pub phase_id: PhaseId,
    pub cancelled_at: Option<String>,
    pub cancellation_reason: Option<String>,
    pub cancelled_by: Option<String>,
    pub position: i64,
    pub title: String,
    pub description: String,
    pub priority: Priority,
    pub progress: Progress,
    pub status_note: String,
    pub owner_actor: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: Version,
    pub agent_type: String,
    pub blocked: bool,
    pub blockers: Vec<Blocker>,
    pub finish_blockers: Vec<Blocker>,
    pub review_gates: Vec<ReviewGate>,
    pub can_finish: bool,
    pub ready: bool,
    pub open_question_count: i64,
    pub blocking_impact: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: QuestionId,
    pub key: String,
    pub board_id: BoardId,
    pub phase_id: PhaseId,
    pub stakeholder_id: Option<String>,
    pub kind: QuestionKind,
    pub title: String,
    pub body: String,
    pub status: QuestionStatus,
    pub answer: String,
    pub owner: String,
    pub created_at: String,
    pub answered_at: Option<String>,
    pub version: Version,
    pub answer_note: String,
    pub recommended_option: String,
    pub recommendation_reason: String,
    pub options: QuestionOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct QuestionOptions(Vec<String>);

impl QuestionOptions {
    pub fn new(raw: &[String]) -> Result<Self> {
        if !(2..=10).contains(&raw.len()) {
            return Err(Error::new(
                "validation_error",
                "a question needs 2 to 10 choices in addition to Other",
            ));
        }
        let mut seen = HashSet::new();
        let mut options = Vec::with_capacity(raw.len() + 1);
        for option in raw {
            let trimmed = option.trim();
            if trimmed.is_empty() {
                return Err(Error::new("validation_error", "option cannot be blank"));
            }
            if trimmed.eq_ignore_ascii_case("other") {
                return Err(Error::new(
                    "validation_error",
                    "Other is added automatically; omit it from options",
                ));
            }
            if trimmed.chars().count() > 200 || !seen.insert(trimmed.to_lowercase()) {
                return Err(Error::new(
                    "validation_error",
                    "options must be distinct and at most 200 characters each",
                ));
            }
            options.push(trimmed.to_owned());
        }
        options.push("Other".to_owned());
        Ok(Self(options))
    }
}

impl TryFrom<Vec<String>> for QuestionOptions {
    type Error = Error;
    fn try_from(options: Vec<String>) -> Result<Self> {
        if options.len() < 2 || options.last().is_none_or(|last| last != "Other") {
            return Err(Error::new(
                "database_error",
                "stored question choices must end with Other",
            ));
        }
        Ok(Self(options))
    }
}
impl From<QuestionOptions> for Vec<String> {
    fn from(value: QuestionOptions) -> Self {
        value.0
    }
}
impl Deref for QuestionOptions {
    type Target = [String];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionOutcome {
    Answered {
        selection: String,
        explanation: String,
    },
    Dropped {
        reason: String,
    },
}

impl QuestionOutcome {
    pub fn answer(options: &[String], selection: &str, explanation: Option<&str>) -> Result<Self> {
        if !options.iter().any(|option| option == selection) {
            return Err(Error::new(
                "validation_error",
                "answer must match one of the question's options",
            )
            .hint("Read the question's options and select one exactly"));
        }
        let explanation = explanation.unwrap_or("").trim();
        if selection == "Other" && explanation.is_empty() {
            return Err(Error::new(
                "validation_error",
                "Other requires an explanation",
            ));
        }
        Ok(Self::Answered {
            selection: selection.to_owned(),
            explanation: explanation.to_owned(),
        })
    }
    pub fn drop(reason: &str) -> Result<Self> {
        if reason.trim().is_empty() {
            return Err(Error::new("validation_error", "reason cannot be blank"));
        }
        Ok(Self::Dropped {
            reason: reason.trim().to_owned(),
        })
    }
}
