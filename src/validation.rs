// LNP/BP Core Library implementing LNPBP specifications & standards
// Written in 2020 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the MIT License
// along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

use core::iter::FromIterator;
use core::ops::AddAssign;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bitcoin::{Transaction, Txid};
use commit_verify::CommitConceal;
use wallet::resolvers::TxResolver;

use super::schema::{NodeType, OccurrencesError};
use super::{
    schema, seal, Anchor, AnchorId, AssignmentVec, Consignment, ContractId,
    Node, NodeId, Schema, SchemaId,
};
use crate::schema::SchemaVerify;
use crate::script::{Action, EntryPoint};
use crate::{SealEndpoint, VmType};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Display)]
#[display(Debug)]
#[repr(u8)]
pub enum Validity {
    Valid,
    UnresolvedTransactions,
    Invalid,
}

#[derive(Clone, Debug, Display, Default, StrictEncode, StrictDecode)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate")
)]
// TODO #42: Display via YAML
#[display(Debug)]
pub struct Status {
    pub unresolved_txids: Vec<Txid>,
    pub failures: Vec<Failure>,
    pub warnings: Vec<Warning>,
    pub info: Vec<Info>,
}

impl AddAssign for Status {
    fn add_assign(&mut self, rhs: Self) {
        self.unresolved_txids.extend(rhs.unresolved_txids);
        self.failures.extend(rhs.failures);
        self.warnings.extend(rhs.warnings);
        self.info.extend(rhs.info);
    }
}

// TODO #43: (new) With rust `try_trait` stabilization re-implement using
//       `Try` trait
// impl Try for Status {
//    type Ok = Status;
//    type Error = Failure;
//    pub fn into_result(self) -> Result<Self::Ok, Self::Error> {
//        unimplemented!()
//    }
//    pub fn from_ok(v: Self::Ok) -> Self {
//        v
//    }
impl Status {
    pub fn from_error(v: Failure) -> Self {
        Status {
            unresolved_txids: vec![],
            failures: vec![v],
            warnings: vec![],
            info: vec![],
        }
    }
}

impl FromIterator<Failure> for Status {
    fn from_iter<T: IntoIterator<Item = Failure>>(iter: T) -> Self {
        Self {
            failures: iter.into_iter().collect(),
            ..Self::default()
        }
    }
}

impl Status {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_failure(failure: Failure) -> Self {
        Self {
            failures: vec![failure],
            ..Self::default()
        }
    }

    pub fn add_failure(&mut self, failure: Failure) -> &Self {
        self.failures.push(failure);
        self
    }

    pub fn add_warning(&mut self, warning: Warning) -> &Self {
        self.warnings.push(warning);
        self
    }

    pub fn add_info(&mut self, info: Info) -> &Self {
        self.info.push(info);
        self
    }

    pub fn validity(&self) -> Validity {
        if !self.failures.is_empty() {
            Validity::Invalid
        } else if !self.unresolved_txids.is_empty() {
            Validity::UnresolvedTransactions
        } else {
            Validity::Valid
        }
    }
}

#[derive(
    Clone, PartialEq, Eq, Debug, Display, From, StrictEncode, StrictDecode,
)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate")
)]
// TODO #44: (v0.3) convert to detailed error description using doc_comments
#[display(Debug)]
pub enum Failure {
    SchemaUnknown(SchemaId),
    /// schema is a subschema, so root schema {0} must be provided for the
    /// validation
    SchemaRootRequired(SchemaId),
    /// Root schema for this schema has another root, which is prohibited
    SchemaRootHierarchy(SchemaId),
    SchemaRootNoFieldTypeMatch(schema::FieldType),
    SchemaRootNoOwnedRightTypeMatch(schema::OwnedRightType),
    SchemaRootNoPublicRightTypeMatch(schema::PublicRightType),
    SchemaRootNoTransitionTypeMatch(schema::TransitionType),
    SchemaRootNoExtensionTypeMatch(schema::ExtensionType),

    SchemaRootNoMetadataMatch(NodeType, schema::FieldType),
    SchemaRootNoParentOwnedRightsMatch(NodeType, schema::OwnedRightType),
    SchemaRootNoParentPublicRightsMatch(NodeType, schema::PublicRightType),
    SchemaRootNoOwnedRightsMatch(NodeType, schema::OwnedRightType),
    SchemaRootNoPublicRightsMatch(NodeType, schema::PublicRightType),
    SchemaRootNoAbiMatch {
        node_type: NodeType,
        action_id: Action,
    },

    SchemaUnknownExtensionType(NodeId, schema::ExtensionType),
    SchemaUnknownTransitionType(NodeId, schema::TransitionType),
    SchemaUnknownFieldType(NodeId, schema::FieldType),
    SchemaUnknownOwnedRightType(NodeId, schema::OwnedRightType),
    SchemaUnknownPublicRightType(NodeId, schema::PublicRightType),

    SchemaDeniedScriptExtension(NodeId),

    SchemaMetaValueTooSmall(schema::FieldType),
    SchemaMetaValueTooLarge(schema::FieldType),
    SchemaStateValueTooSmall(schema::OwnedRightType),
    SchemaStateValueTooLarge(schema::OwnedRightType),

    SchemaMismatchedBits {
        field_or_state_type: u16,
        expected: schema::Bits,
    },
    SchemaWrongEnumValue {
        field_or_state_type: u16,
        unexpected: u8,
    },
    SchemaWrongDataLength {
        field_or_state_type: u16,
        max_expected: u16,
        found: usize,
    },
    SchemaMismatchedDataType(u16),
    SchemaMismatchedStateType(schema::OwnedRightType),

    SchemaMetaOccurrencesError(NodeId, schema::FieldType, OccurrencesError),
    SchemaParentOwnedRightOccurrencesError(
        NodeId,
        schema::OwnedRightType,
        OccurrencesError,
    ),
    SchemaOwnedRightOccurrencesError(
        NodeId,
        schema::OwnedRightType,
        OccurrencesError,
    ),

    SchemaScriptOverrideDenied,
    SchemaScriptVmChangeDenied,

    TransitionAbsent(NodeId),
    TransitionNotAnchored(NodeId),
    TransitionNotInAnchor(NodeId, AnchorId),
    TransitionParentWrongSealType {
        node_id: NodeId,
        ancestor_id: NodeId,
        assignment_type: schema::OwnedRightType,
    },
    TransitionParentWrongSeal {
        node_id: NodeId,
        ancestor_id: NodeId,
        assignment_type: schema::OwnedRightType,
        seal_index: u16,
    },
    TransitionParentConfidentialSeal {
        node_id: NodeId,
        ancestor_id: NodeId,
        assignment_type: schema::OwnedRightType,
        seal_index: u16,
    },
    TransitionParentIsNotWitnessInput {
        node_id: NodeId,
        ancestor_id: NodeId,
        assignment_type: schema::OwnedRightType,
        seal_index: u16,
        outpoint: bitcoin::OutPoint,
    },

    ExtensionAbsent(NodeId),
    ExtensionParentWrongValenciesType {
        node_id: NodeId,
        ancestor_id: NodeId,
        valencies_type: schema::PublicRightType,
    },

    WitnessTransactionMissed(Txid),
    WitnessNoCommitment(NodeId, AnchorId, Txid),

    EndpointTransitionNotFound(NodeId),

    /// invalid bulletproofs in {0}:{1}: {2}
    InvalidBulletproofs(NodeId, u16, secp256k1zkp::Error),

    WrongEntryPoint(EntryPoint),
    /// Under certain conditions the script code must be empty, for instance if
    /// you use the embedded virtual machine or subschema which does not
    /// override the parent scripts
    ScriptCodeMustBeEmpty,
    VirtualMachinesNotSupportedYet,
    ScriptFailure(NodeId, u8),
}

#[derive(
    Clone, PartialEq, Eq, Debug, Display, From, StrictEncode, StrictDecode,
)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate")
)]
// TODO #44: (v0.3) convert to detailed descriptions using doc_comments
#[display(Debug)]
pub enum Warning {
    EndpointDuplication(NodeId, SealEndpoint),
    EndpointTransitionSealNotFound(NodeId, SealEndpoint),
    ExcessiveNode(NodeId),
}

#[derive(
    Clone, PartialEq, Eq, Debug, Display, From, StrictEncode, StrictDecode,
)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(crate = "serde_crate")
)]
// TODO #44: (v0.3) convert to detailed descriptions using doc_comments
#[display(Debug)]
pub enum Info {
    UncheckableConfidentialStateData(NodeId, u16),
}

pub struct Validator<'validator, R: TxResolver> {
    consignment: &'validator Consignment,

    status: Status,

    schema_id: SchemaId,
    genesis_id: NodeId,
    contract_id: ContractId,
    node_index: BTreeMap<NodeId, &'validator dyn Node>,
    anchor_index: BTreeMap<NodeId, &'validator Anchor>,
    end_transitions: Vec<&'validator dyn Node>,
    validation_index: BTreeSet<NodeId>,

    resolver: R,
}

impl<'validator, R: TxResolver> Validator<'validator, R> {
    fn init(consignment: &'validator Consignment, resolver: R) -> Self {
        // We use validation status object to store all detected failures and
        // warnings
        let mut status = Status::default();

        // Frequently used computation-heavy data
        let genesis_id = consignment.genesis.node_id();
        let contract_id = consignment.genesis.contract_id();
        let schema_id = consignment.genesis.schema_id();

        // Create indexes
        let mut node_index = BTreeMap::<NodeId, &dyn Node>::new();
        let mut anchor_index = BTreeMap::<NodeId, &Anchor>::new();
        for (anchor, transition) in &consignment.state_transitions {
            let node_id = transition.node_id();
            node_index.insert(node_id, transition);
            anchor_index.insert(node_id, anchor);
        }
        node_index.insert(genesis_id, &consignment.genesis);
        for extension in &consignment.state_extensions {
            let node_id = extension.node_id();
            node_index.insert(node_id, extension);
        }

        // Collect all endpoint transitions
        // This is pretty simple operation; it takes a lot of code because
        // we would like to detect any potential issues with the consignment
        // structure and notify user about them (in form of generated warnings)
        let mut end_transitions = Vec::<&dyn Node>::new();
        for (node_id, seal_endpoint) in &consignment.endpoints {
            if let Some(node) = node_index.get(node_id) {
                // Checking for endpoint definition duplicates
                if node
                    .to_confiential_seals()
                    .contains(&seal_endpoint.commit_conceal())
                {
                    if end_transitions
                        .iter()
                        .filter(|n| n.node_id() == *node_id)
                        .collect::<Vec<_>>()
                        .len()
                        > 0
                    {
                        status.add_warning(Warning::EndpointDuplication(
                            *node_id,
                            *seal_endpoint,
                        ));
                    } else {
                        end_transitions.push(*node);
                    }
                } else {
                    // We generate just a warning here because it's up to a user
                    // to decide whether to accept consignment with wrong
                    // endpoint list
                    status.add_warning(
                        Warning::EndpointTransitionSealNotFound(
                            *node_id,
                            *seal_endpoint,
                        ),
                    );
                }
            } else {
                // ~~We generate just a warning here because it's up to a user
                // to decide whether to accept consignment with wrong
                // endpoint list~~
                // Update: warning is transformed into an error, since it may
                // lead to acceptance of non-verified consignment assigning
                // positive fake balance to the user-controlled UTXO
                status
                    .add_failure(Failure::EndpointTransitionNotFound(*node_id));
            }
        }

        // Validation index is used to check that all transitions presented
        // in the consignment were validated. Also, we use it to avoid double
        // schema validations for transitions.
        let validation_index = BTreeSet::<NodeId>::new();

        Self {
            consignment,
            status,
            schema_id,
            genesis_id,
            contract_id,
            node_index,
            anchor_index,
            end_transitions,
            validation_index,
            resolver,
        }
    }

    /// Validation procedure takes a schema object, root schema (if any),
    /// resolver function returning transaction and its fee for a given
    /// transaction id, and returns a validation object listing all detected
    /// failures, warnings and additional information.
    ///
    /// When a failure detected, it not stopped; the failure is is logged into
    /// the status object, but the validation continues for the rest of the
    /// consignment data. This can help it debugging and detecting all problems
    /// with the consignment.
    pub fn validate(
        schema: &Schema,
        root: Option<&Schema>,
        consignment: &'validator Consignment,
        resolver: R,
    ) -> Status {
        let mut validator = Validator::init(consignment, resolver);

        validator.validate_schema(schema, root);
        // We must return here, since if the schema is not valid there is no
        // reason to validate contract nodes against it: it will produce a
        // plenty of errors
        if validator.status.validity() == Validity::Invalid {
            return validator.status;
        }

        let byte_code = if schema.script.byte_code.is_empty() {
            root.map(|root| &root.script.byte_code)
                .unwrap_or(&schema.script.byte_code)
        } else {
            &schema.script.byte_code
        };

        validator.validate_contract(schema, byte_code);

        // Done. Returning status report with all possible failures, issues,
        // warnings and notifications about transactions we were unable to
        // obtain.
        validator.status
    }

    fn validate_schema(&mut self, schema: &Schema, root: Option<&Schema>) {
        // Validating schema against root schema
        if let Some(root) = root {
            self.status += schema.schema_verify(root);
        } else if schema.root_id != default!() {
            self.status
                .add_failure(Failure::SchemaRootRequired(schema.root_id));
        }

        // Validating VM
        if schema.script.vm_type != VmType::Embedded {
            self.status
                .add_failure(Failure::VirtualMachinesNotSupportedYet);
        }
    }

    fn validate_contract(&mut self, schema: &Schema, byte_code: &[u8]) {
        // [VALIDATION]: Making sure that we were supplied with the schema
        //               that corresponds to the schema of the contract genesis
        if schema.schema_id() != self.schema_id {
            self.status
                .add_failure(Failure::SchemaUnknown(self.schema_id));
            // Unlike other failures, here we return immediatelly, since there
            // is no point to validate all consignment data against an invalid
            // schema: it will result in a plenty of meaningless errors
            return;
        }

        // [VALIDATION]: Validate genesis
        self.status += schema.validate(
            &self.node_index,
            &self.consignment.genesis,
            byte_code,
        );
        self.validation_index.insert(self.genesis_id);

        // [VALIDATION]: Iterating over each endpoint, reconstructing node graph
        //               up to genesis for each one of them. NB: We are not
        //               aiming to validate the consignment as a whole, but
        //               instead treat it as a superposition of subgraphs, one
        //               for each endpoint; and validate them independently.
        for node in self.end_transitions.clone() {
            self.validate_branch(schema, node, byte_code);
        }

        // Generate warning if some of the transitions within the consignment
        // were excessive (i.e. not part of validation_index). Nothing critical,
        // but still good to report the user that the consignment is not perfect
        for node_id in self
            .validation_index
            .difference(&self.consignment.node_ids())
        {
            self.status.add_warning(Warning::ExcessiveNode(*node_id));
        }
    }

    fn validate_branch(
        &mut self,
        schema: &Schema,
        node: &'validator dyn Node,
        byte_code: &[u8],
    ) {
        let mut queue: VecDeque<&dyn Node> = VecDeque::new();

        // Instead of constructing complex graph structures or using a
        // recursions we utilize queue to keep the track of the upstream
        // (ancestor) nodes and make sure that ve have validated each one
        // of them up to genesis. The graph is valid when each of its nodes
        // and each of its edges is valid, i.e. when all individual nodes
        // has passed validation against the schema (we track that fact with
        // `validation_index`) and each of the node ancestor state change to
        // a given node is valid against the schema + committed into bitcoin
        // transaction graph with proper anchor. That is what we are
        // checking in the code below:
        queue.push_back(node);
        while let Some(node) = queue.pop_front() {
            let node_id = node.node_id();
            let node_type = node.node_type();

            // [VALIDATION]: Verify node against the schema. Here we check
            //               only a single node, not state evolution (it
            //               will be checked lately)
            if !self.validation_index.contains(&node_id) {
                self.status +=
                    schema.validate(&self.node_index, node, byte_code);
                self.validation_index.insert(node_id);
            }

            // Making sure we do have a corresponding anchor; otherwise
            // reporting failure (see below) - with the except of genesis and
            // extension nodes, which does not have a corresponding anchor
            if let Some(anchor) = self.anchor_index.get(&node_id).cloned() {
                // Ok, now we have the `node` and the `anchor`, let's do all
                // required checks

                // [VALIDATION]: Check that transition is committed into the
                //               anchor. This must be done with
                //               deterministic bitcoin commitments & LNPBP-4
                if !anchor.validate(&self.contract_id, &node_id) {
                    self.status.add_failure(Failure::TransitionNotInAnchor(
                        node_id,
                        anchor.anchor_id(),
                    ));
                }

                self.validate_graph_node(node, anchor);

            // Ouch, we are out of that multi-level nested cycles :)
            } else if node_type != NodeType::Genesis
                && node_type != NodeType::StateExtension
            {
                // This point is actually unreachable: b/c of the
                // consignment structure, each state transition
                // has a corresponding anchor. So if we've got here there
                // is something broken with LNP/BP core library.
                self.status
                    .add_failure(Failure::TransitionNotAnchored(node_id));
            }

            // Now, we must collect all parent nodes and add them to the
            // verification queue
            let parent_nodes_1: Vec<&dyn Node> = node
                .parent_owned_rights()
                .iter()
                .filter_map(|(id, _)| {
                    self.node_index.get(id).cloned().or_else(|| {
                        // This will not actually happen since we already
                        // checked that each ancrstor reference has a
                        // corresponding node in the code above. But rust
                        // requires to double-check :)
                        self.status.add_failure(Failure::TransitionAbsent(*id));
                        None
                    })
                })
                .collect();

            let parent_nodes_2: Vec<&dyn Node> = node
                .parent_public_rights()
                .iter()
                .filter_map(|(id, _)| {
                    self.node_index.get(id).cloned().or_else(|| {
                        // This will not actually happen since we already
                        // checked that each ancrstor reference has a
                        // corresponding node in the code above. But rust
                        // requires to double-check :)
                        self.status.add_failure(Failure::TransitionAbsent(*id));
                        None
                    })
                })
                .collect();

            queue.extend(parent_nodes_1);
            queue.extend(parent_nodes_2);
        }
    }

    fn validate_graph_node(
        &mut self,
        node: &'validator dyn Node,
        anchor: &'validator Anchor,
    ) {
        let txid = anchor.txid;
        let node_id = node.node_id();

        // Check that the anchor is committed into a transaction spending all of
        // the transition inputs.
        match self.resolver.resolve(&txid) {
            Err(_) => {
                // We wre unable to retrieve corresponding transaction, so can't
                // check. Reporting this incident and continuing further.
                // Why this happens? no connection to Bitcoin Core, Electrum or
                // other backend etc. So this is not a failure in a strict
                // sense, however we can't be sure that the
                // consignment is valid. That's why we keep the
                // track of such information in a separate place
                // (`unresolved_txids` field of the validation
                // status object).
                self.status.unresolved_txids.push(txid);
            }
            Ok(None) => {
                // There is no mined transaction with the id provided by the
                // anchor. Literally, the whole consignment is fucked up, but we
                // are proceeding with further validation in order to detect the
                // rest of fuck ups (and reporting the failure!)
                self.status
                    .add_failure(Failure::WitnessTransactionMissed(txid));
            }
            Ok(Some((witness_tx, fee))) => {
                // Ok, now we have the transaction and fee information for a
                // single state change from some ancestors array to the
                // currently validated transition node: that's everything
                // required to do the complete validation

                // [VALIDATION]: Checking anchor deterministic bitcoin
                //               commitment
                if !anchor.verify(&self.contract_id, &witness_tx, fee) {
                    // The node is not committed to bitcoin transaction graph!
                    // Ultimate failure. But continuing to detect the rest
                    // (after reporting it).
                    self.status.add_failure(Failure::WitnessNoCommitment(
                        node_id,
                        anchor.anchor_id(),
                        txid,
                    ));
                }

                // Checking that bitcoin transaction closes seals defined by
                // transition ancestors.
                for (ancestor_id, assignments) in
                    node.parent_owned_rights().iter()
                {
                    let ancestor_id = *ancestor_id;
                    let ancestor_node = if let Some(ancestor_node) =
                        self.node_index.get(&ancestor_id)
                    {
                        *ancestor_node
                    } else {
                        // Node, referenced as the ancestor, was not found
                        // in the consignment. Usually this means that the
                        // consignment data are broken
                        self.status.add_failure(Failure::TransitionAbsent(
                            ancestor_id,
                        ));
                        continue;
                    };

                    for (assignment_type, assignment_indexes) in assignments {
                        let assignment_type = *assignment_type;

                        let variant = if let Some(variant) =
                            ancestor_node.owned_rights_by_type(assignment_type)
                        {
                            variant
                        } else {
                            self.status.add_failure(
                                Failure::TransitionParentWrongSealType {
                                    node_id,
                                    ancestor_id,
                                    assignment_type,
                                },
                            );
                            continue;
                        };

                        for seal_index in assignment_indexes {
                            self.validate_witness_input(
                                &witness_tx,
                                node_id,
                                ancestor_id,
                                assignment_type,
                                variant,
                                *seal_index,
                            );
                        }
                    }
                }
            }
        }
    }

    // TODO #45: Move part of logic into single-use-seals and bitcoin seals
    fn validate_witness_input(
        &mut self,
        witness_tx: &Transaction,
        node_id: NodeId,
        ancestor_id: NodeId,
        assignment_type: schema::OwnedRightType,
        variant: &'validator AssignmentVec,
        seal_index: u16,
    ) {
        // Getting bitcoin transaction outpoint for the current ancestor ... ->
        match (
            variant.revealed_seal_at(seal_index),
            self.anchor_index.get(&ancestor_id),
        ) {
            (Err(_), _) => {
                self.status.add_failure(Failure::TransitionParentWrongSeal {
                    node_id,
                    ancestor_id,
                    assignment_type,
                    seal_index,
                });
                None
            }
            (Ok(None), _) => {
                // Everything is ok, but we have incomplete data (confidential),
                // thus can't do a full verification and have to report the
                // failure
                eprintln!("{:#?}", variant);
                self.status.add_failure(
                    Failure::TransitionParentConfidentialSeal {
                        node_id,
                        ancestor_id,
                        assignment_type,
                        seal_index,
                    },
                );
                None
            }
            (Ok(Some(seal::Revealed::TxOutpoint(outpoint))), None) => {
                // We are at genesis, so the outpoint must contain tx
                Some(bitcoin::OutPoint::from(outpoint.clone()))
            }
            (Ok(Some(_)), None) => {
                // This can't happen, since if we have a node in the index
                // and the node is not genesis, we always have an anchor
                unreachable!()
            }
            (Ok(Some(seal)), Some(anchor)) => Some(bitcoin::OutPoint::from(
                seal.to_outpoint_reveal(anchor.txid),
            )), /* -> ... so we can check that the bitcoin transaction
                 * references it as one of its inputs */
        }
        .map(|outpoint| {
            if witness_tx
                .input
                .iter()
                .find(|txin| txin.previous_output == outpoint)
                .is_none()
            {
                // Another failure: we do not spend one of the transition
                // ancestors in the witness transaction. The consignment is
                // clearly invalid; reporting this and processing to other
                // potential issues.
                self.status.add_failure(
                    Failure::TransitionParentIsNotWitnessInput {
                        node_id,
                        ancestor_id,
                        assignment_type,
                        seal_index,
                        outpoint,
                    },
                );
            }
        });
    }
}
