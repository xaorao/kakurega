use cedar_policy::{
    Authorizer, Context, Decision as CedarDecision, Entities, Entity, EntityId, EntityTypeName,
    EntityUid, PolicySet as CedarPolicySet, Request as CedarRequest, RestrictedExpression, Schema,
    ValidationMode, Validator,
};
use kk_types::label::{Integrity, Label, Level};
use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::str::FromStr;

pub const DEFAULT_SCHEMA: &str = include_str!("../policy/schema.cedarschema");
pub const DEFAULT_POLICY: &str = include_str!("../policy/default.cedar");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    ReadArtifact,
    WriteArtifact,
    ModelComplete,
    ToolCall,
    ReleasePropose,
}

impl Op {
    pub fn name(self) -> &'static str {
        match self {
            Op::ReadArtifact => "read.artifact",
            Op::WriteArtifact => "write.artifact",
            Op::ModelComplete => "model.complete",
            Op::ToolCall => "tool.call",
            Op::ReleasePropose => "release.propose",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerFacts {
    pub id: String,
    pub kind: String,
    pub clearance: Label,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resource {
    Artifact {
        id: String,
        label: Label,
    },
    Tool {
        id: String,
        readonly: bool,
    },
    Route {
        id: String,
        class: String,
        tee_verified: bool,
    },
    Recipient {
        id: String,
        clearance_level: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepFacts {
    pub taint: Label,
    pub human_approved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenyReason {
    NoMatchingPermit,
    Forbidden(Vec<String>),
    EvaluationError(Vec<String>),
    InvalidRequest(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow { policies: Vec<String> },
    Deny(DenyReason),
}

impl Decision {
    pub fn is_allow(&self) -> bool {
        matches!(self, Decision::Allow { .. })
    }
}

#[derive(Debug)]
pub enum PolicyError {
    Schema(String),
    Policy(String),
    Validation(Vec<String>),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyError::Schema(m) => write!(f, "schema error: {m}"),
            PolicyError::Policy(m) => write!(f, "policy error: {m}"),
            PolicyError::Validation(v) => write!(f, "policy validation failed: {}", v.join("; ")),
        }
    }
}

impl std::error::Error for PolicyError {}

pub struct Engine {
    policies: CedarPolicySet,
    schema: Schema,
    authorizer: Authorizer,
}

fn level_number(l: Level) -> i64 {
    match l {
        Level::Public => 0,
        Level::Internal => 1,
        Level::Confidential => 2,
        Level::Restricted => 3,
    }
}

fn uid(kind: &str, id: &str) -> Result<EntityUid, String> {
    let ty = EntityTypeName::from_str(kind).map_err(|e| e.to_string())?;
    Ok(EntityUid::from_type_name_and_id(ty, EntityId::new(id)))
}

fn string_set(items: impl IntoIterator<Item = String>) -> RestrictedExpression {
    RestrictedExpression::new_set(items.into_iter().map(RestrictedExpression::new_string))
}

impl Engine {
    pub fn new(policy_text: &str, schema_text: &str) -> Result<Engine, PolicyError> {
        let schema = Schema::from_cedarschema_str(schema_text)
            .map_err(|e| PolicyError::Schema(e.to_string()))?
            .0;
        let policies = CedarPolicySet::from_str(policy_text)
            .map_err(|e| PolicyError::Policy(e.to_string()))?;
        let result = Validator::new(schema.clone()).validate(&policies, ValidationMode::Strict);
        if !result.validation_passed() {
            return Err(PolicyError::Validation(
                result.validation_errors().map(|e| e.to_string()).collect(),
            ));
        }
        Ok(Engine {
            policies,
            schema,
            authorizer: Authorizer::new(),
        })
    }

    pub fn default_policy() -> Engine {
        Engine::new(DEFAULT_POLICY, DEFAULT_SCHEMA)
            .expect("the built-in policy validates against the built-in schema")
    }

    pub fn decide(
        &self,
        worker: &WorkerFacts,
        op: Op,
        resource: &Resource,
        step: &StepFacts,
    ) -> Decision {
        match self.try_decide(worker, op, resource, step) {
            Ok(d) => d,
            Err(m) => Decision::Deny(DenyReason::InvalidRequest(m)),
        }
    }

    fn try_decide(
        &self,
        worker: &WorkerFacts,
        op: Op,
        resource: &Resource,
        step: &StepFacts,
    ) -> Result<Decision, String> {
        let worker_uid = uid("Worker", &worker.id)?;
        let mut tool_uids = Vec::new();
        let mut tool_entities = Vec::new();
        for t in &worker.allowed_tools {
            let u = uid("Tool", t)?;
            tool_uids.push(u.clone());
            tool_entities.push(u);
        }
        let worker_entity = Entity::new(
            worker_uid.clone(),
            [
                (
                    "kind".to_string(),
                    RestrictedExpression::new_string(worker.kind.clone()),
                ),
                (
                    "clearance_level".to_string(),
                    RestrictedExpression::new_long(level_number(
                        worker.clearance.effective_level(),
                    )),
                ),
                (
                    "compartments".to_string(),
                    string_set(worker.clearance.compartments.iter().cloned()),
                ),
                (
                    "allowed_tools".to_string(),
                    RestrictedExpression::new_set(
                        tool_uids
                            .iter()
                            .map(|u| RestrictedExpression::new_entity_uid(u.clone())),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
            HashSet::new(),
        )
        .map_err(|e| e.to_string())?;

        let mut entities = vec![worker_entity];
        let resource_uid = match resource {
            Resource::Artifact { id, label } => {
                let u = uid("Artifact", id)?;
                entities.push(
                    Entity::new(
                        u.clone(),
                        [
                            (
                                "level".to_string(),
                                RestrictedExpression::new_long(level_number(label.level)),
                            ),
                            (
                                "compartments".to_string(),
                                string_set(label.compartments.iter().cloned()),
                            ),
                            (
                                "untrusted".to_string(),
                                RestrictedExpression::new_bool(
                                    label.integrity == Integrity::Untrusted,
                                ),
                            ),
                            (
                                "sensitivity_unknown".to_string(),
                                RestrictedExpression::new_bool(label.sensitivity_unknown),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                        HashSet::new(),
                    )
                    .map_err(|e| e.to_string())?,
                );
                u
            }
            Resource::Tool { id, readonly } => {
                let u = uid("Tool", id)?;
                entities.push(
                    Entity::new(
                        u.clone(),
                        [(
                            "readonly".to_string(),
                            RestrictedExpression::new_bool(*readonly),
                        )]
                        .into_iter()
                        .collect(),
                        HashSet::new(),
                    )
                    .map_err(|e| e.to_string())?,
                );
                u
            }
            Resource::Route {
                id,
                class,
                tee_verified,
            } => {
                let u = uid("Route", id)?;
                entities.push(
                    Entity::new(
                        u.clone(),
                        [
                            (
                                "class".to_string(),
                                RestrictedExpression::new_string(class.clone()),
                            ),
                            (
                                "tee_verified".to_string(),
                                RestrictedExpression::new_bool(*tee_verified),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                        HashSet::new(),
                    )
                    .map_err(|e| e.to_string())?,
                );
                u
            }
            Resource::Recipient {
                id,
                clearance_level,
            } => {
                let u = uid("Recipient", id)?;
                entities.push(
                    Entity::new(
                        u.clone(),
                        [(
                            "clearance_level".to_string(),
                            RestrictedExpression::new_long(*clearance_level),
                        )]
                        .into_iter()
                        .collect(),
                        HashSet::new(),
                    )
                    .map_err(|e| e.to_string())?,
                );
                u
            }
        };
        for u in tool_entities {
            if resource_uid != u && !entities.iter().any(|e| e.uid() == u) {
                entities.push(
                    Entity::new(
                        u,
                        [(
                            "readonly".to_string(),
                            RestrictedExpression::new_bool(false),
                        )]
                        .into_iter()
                        .collect(),
                        HashSet::new(),
                    )
                    .map_err(|e| e.to_string())?,
                );
            }
        }
        let entities =
            Entities::from_entities(entities, Some(&self.schema)).map_err(|e| e.to_string())?;
        let context = Context::from_pairs([
            (
                "taint_level".to_string(),
                RestrictedExpression::new_long(level_number(step.taint.effective_level())),
            ),
            (
                "taint_untrusted".to_string(),
                RestrictedExpression::new_bool(step.taint.integrity == Integrity::Untrusted),
            ),
            (
                "human_approved".to_string(),
                RestrictedExpression::new_bool(step.human_approved),
            ),
        ])
        .map_err(|e| e.to_string())?;
        let action = uid("Action", op.name())?;
        let request = CedarRequest::new(
            worker_uid,
            action,
            resource_uid,
            context,
            Some(&self.schema),
        )
        .map_err(|e| e.to_string())?;
        let response = self
            .authorizer
            .is_authorized(&request, &self.policies, &entities);
        let errors: Vec<String> = response
            .diagnostics()
            .errors()
            .map(|e| e.to_string())
            .collect();
        if !errors.is_empty() {
            return Ok(Decision::Deny(DenyReason::EvaluationError(errors)));
        }
        let reasons: BTreeSet<String> = response
            .diagnostics()
            .reason()
            .map(|p| p.to_string())
            .collect();
        Ok(match response.decision() {
            CedarDecision::Allow => Decision::Allow {
                policies: reasons.into_iter().collect(),
            },
            CedarDecision::Deny if reasons.is_empty() => {
                Decision::Deny(DenyReason::NoMatchingPermit)
            }
            CedarDecision::Deny => {
                Decision::Deny(DenyReason::Forbidden(reasons.into_iter().collect()))
            }
        })
    }
}
