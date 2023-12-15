// Copyright Materialize, Inc. and contributors. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

//! Notices that the optimizer wants to show to users.
//!
//! The top-level notice types are [`RawOptimizerNotice`] (for notices emitted
//! by optimizer pipelines) and [`OptimizerNotice`] (for notices stored in the
//! catalog memory). The `adapter` module contains code for converting the
//! former to the latter.
//!
//! The [`RawOptimizerNotice`] type is an enum generated by the
//! `raw_optimizer_notices` macro. Each notice type lives in its own submodule
//! and implements the [`OptimizerNoticeApi`] trait.
//!
//! To add a new notice do the following:
//!
//! 1. Create a new submodule.
//! 2. Define a struct for the new notice in that submodule.
//! 3. Implement [`OptimizerNoticeApi`] for that struct.
//! 4. Re-export the notice type in this module.
//! 5. Add the notice type to the `raw_optimizer_notices` macro which generates
//!    the [`RawOptimizerNotice`] enum and other boilerplate code.

// Modules (one for each notice type).
mod index_key_empty;
mod index_too_wide_for_literal_constraints;

pub use index_key_empty::IndexKeyEmpty;
pub use index_too_wide_for_literal_constraints::IndexTooWideForLiteralConstraints;

use std::collections::BTreeSet;
use std::fmt::{self, Error, Formatter, Write};
use std::sync::Arc;
use std::{concat, stringify};

use enum_kinds::EnumKind;
use mz_repr::explain::ExprHumanizer;
use mz_repr::GlobalId;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
/// An long lived in-memory representation of a [`RawOptimizerNotice`] that is
/// meant to be kept as part of the hydrated catalog state.
pub struct OptimizerNotice {
    /// The notice kind.
    pub kind: OptimizerNoticeKind,
    /// The ID of the catalog item associated with this notice.
    ///
    /// This is `None` if the notice is scoped to the entire catalog.
    pub item_id: Option<GlobalId>,
    /// A set of ids that need to exist for this notice to be considered valid.
    /// Removing any of the IDs in this set will result in the notice being
    /// asynchronously removed from the catalog state.
    pub dependencies: BTreeSet<GlobalId>,
    /// A brief description of what concretely went wrong.
    ///
    /// Details and context about situations in which this notice kind would be
    /// emitted should be reserved for the documentation page for this notice
    /// kind.
    pub message: String,
    /// A high-level hint that tells the user what can be improved.
    pub hint: String,
    /// A recommended action. This is a more concrete version of the hint.
    pub action: Action,
    /// A redacted version of the `message` field.
    pub message_redacted: String,
    /// A redacted version of the `hint` field.
    pub hint_redacted: String,
    /// A redacted version of the `action` field.
    pub action_redacted: Action,
    /// The date at which this notice was last created.
    pub created_at: u64,
}

impl OptimizerNotice {
    /// Turns a vector of notices into a vector of strings that can be used in
    /// EXPLAIN.
    ///
    /// This method should be consistent with [`RawOptimizerNotice::explain`].
    pub fn explain(
        notices: &Vec<Arc<Self>>,
        humanizer: &dyn ExprHumanizer,
        redacted: bool,
    ) -> Result<Vec<String>, Error> {
        let mut notice_strings = Vec::new();
        for notice in notices {
            if notice.is_valid(humanizer) {
                let mut s = String::new();
                if redacted {
                    write!(s, "  - Notice: {}\n", notice.message_redacted)?;
                    write!(s, "    Hint: {}", notice.hint_redacted)?;
                } else {
                    write!(s, "  - Notice: {}\n", notice.message)?;
                    write!(s, "    Hint: {}", notice.hint)?;
                };
                notice_strings.push(s);
            }
        }
        Ok(notice_strings)
    }

    /// Returns `true` iff both the dependencies and the associated item for
    /// this notice still exist.
    ///
    /// This method should be consistent with [`RawOptimizerNotice::is_valid`].
    fn is_valid(&self, humanizer: &dyn ExprHumanizer) -> bool {
        // All dependencies exist.
        self.dependencies.iter().all(|id| humanizer.id_exists(*id))
    }
}

#[derive(EnumKind, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[enum_kind(ActionKind)]
/// An action attached to an [`OptimizerNotice`]
pub enum Action {
    /// No action.
    None,
    /// An action that cannot be defined as a program.
    PlainText(String),
    /// One or more SQL statements
    ///
    /// The statements should be formatted and fully-qualified names, meaning
    /// that this field can be rendered in the console with a button that
    /// executes this as a valid SQL statement.
    SqlStatements(String),
}

impl Action {
    /// Return the kind of this notice.
    pub fn kind(&self) -> ActionKind {
        ActionKind::from(self)
    }
}

impl ActionKind {
    /// Return a string representation for this action kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "",
            Self::PlainText => "plain_text",
            Self::SqlStatements => "sql_statements",
        }
    }
}

/// An API structs [`RawOptimizerNotice`] wrapped by structs
pub trait OptimizerNoticeApi: Sized {
    /// See [`OptimizerNoticeApi::dependencies`].
    fn dependencies(&self) -> BTreeSet<GlobalId>;

    /// Format the text for the optionally redacted [`OptimizerNotice::message`]
    /// value for this notice.
    fn fmt_message(
        &self,
        f: &mut Formatter<'_>,
        humanizer: &dyn ExprHumanizer,
        redacted: bool,
    ) -> fmt::Result;

    /// Format the text for the optionally redacted [`OptimizerNotice::hint`]
    /// value for this notice.
    fn fmt_hint(
        &self,
        f: &mut Formatter<'_>,
        humanizer: &dyn ExprHumanizer,
        redacted: bool,
    ) -> fmt::Result;

    /// Format the text for the optionally redacted [`OptimizerNotice::action`]
    /// value for this notice.
    fn fmt_action(
        &self,
        f: &mut Formatter<'_>,
        humanizer: &dyn ExprHumanizer,
        redacted: bool,
    ) -> fmt::Result;

    /// The kind of action suggested by this notice.
    fn action_kind(&self, humanizer: &dyn ExprHumanizer) -> ActionKind;

    /// Return a thunk that will render the optionally redacted
    /// [`OptimizerNotice::message`] value for this notice.
    fn message<'a>(
        &'a self,
        humanizer: &'a dyn ExprHumanizer,
        redacted: bool,
    ) -> HumanizedMessage<'a, Self> {
        HumanizedMessage {
            notice: self,
            humanizer,
            redacted,
        }
    }

    /// Return a thunk that will render the optionally redacted
    /// [`OptimizerNotice::hint`] value for
    /// this notice.
    fn hint<'a>(
        &'a self,
        humanizer: &'a dyn ExprHumanizer,
        redacted: bool,
    ) -> HumanizedHint<'a, Self> {
        HumanizedHint {
            notice: self,
            humanizer,
            redacted,
        }
    }

    /// Return a thunk that will render the optionally redacted
    /// [`OptimizerNotice::action`] value for this notice.
    fn action<'a>(
        &'a self,
        humanizer: &'a dyn ExprHumanizer,
        redacted: bool,
    ) -> HumanizedAction<'a, Self> {
        HumanizedAction {
            notice: self,
            humanizer,
            redacted,
        }
    }
}

/// A wrapper for the [`OptimizerNoticeApi::fmt_message`] that implements
/// [`fmt::Display`].
#[allow(missing_debug_implementations)]
pub struct HumanizedMessage<'a, T> {
    notice: &'a T,
    humanizer: &'a dyn ExprHumanizer,
    redacted: bool,
}
impl<'a, T: OptimizerNoticeApi> fmt::Display for HumanizedMessage<'a, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.notice.fmt_message(f, self.humanizer, self.redacted)
    }
}

/// A wrapper for the [`OptimizerNoticeApi::fmt_hint`] that implements [`fmt::Display`].
#[allow(missing_debug_implementations)]
pub struct HumanizedHint<'a, T> {
    notice: &'a T,
    humanizer: &'a dyn ExprHumanizer,
    redacted: bool,
}

impl<'a, T: OptimizerNoticeApi> fmt::Display for HumanizedHint<'a, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.notice.fmt_hint(f, self.humanizer, self.redacted)
    }
}

/// A wrapper for the [`OptimizerNoticeApi::fmt_action`] that implements
/// [`fmt::Display`].
#[allow(missing_debug_implementations)]
pub struct HumanizedAction<'a, T> {
    notice: &'a T,
    humanizer: &'a dyn ExprHumanizer,
    redacted: bool,
}

impl<'a, T: OptimizerNoticeApi> fmt::Display for HumanizedAction<'a, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.notice.fmt_action(f, self.humanizer, self.redacted)
    }
}

macro_rules! raw_optimizer_notices {
    ($($ty:ident => $name:literal,)+) => {
        paste::paste!{
            /// Notices that the optimizer wants to show to users.
            #[derive(EnumKind, Clone, Debug, Eq, PartialEq)]
            #[enum_kind(OptimizerNoticeKind, derive(PartialOrd, Ord))]
            pub enum RawOptimizerNotice {
                $(
                    #[doc = concat!("See [`", stringify!($ty), "`].")]
                    $ty($ty),
                )+
            }

            impl OptimizerNoticeApi for RawOptimizerNotice {
                fn dependencies(&self) -> BTreeSet<GlobalId> {
                    match self {
                        $(Self::$ty(notice) => notice.dependencies(),)+
                    }
                }

                fn fmt_message(&self, f: &mut Formatter<'_>, humanizer: &dyn ExprHumanizer, redacted: bool) -> fmt::Result {
                    match self {
                        $(Self::$ty(notice) => notice.fmt_message(f, humanizer, redacted),)+
                    }
                }

                fn fmt_hint(&self, f: &mut Formatter<'_>, humanizer: &dyn ExprHumanizer, redacted: bool) -> fmt::Result {
                    match self {
                        $(Self::$ty(notice) => notice.fmt_hint(f, humanizer, redacted),)+
                    }
                }

                fn fmt_action(&self, f: &mut Formatter<'_>, humanizer: &dyn ExprHumanizer, redacted: bool) -> fmt::Result {
                    match self {
                        $(Self::$ty(notice) => notice.fmt_action(f, humanizer, redacted),)+
                    }
                }

                fn action_kind(&self, humanizer: &dyn ExprHumanizer) -> ActionKind {
                    match self {
                        $(Self::$ty(notice) => notice.action_kind(humanizer),)+
                    }
                }
            }

            impl OptimizerNoticeKind {
                /// Return a string representation for this optimizer notice
                /// kind.
                pub fn as_str(&self) -> &'static str {
                    match self {
                        $(Self::$ty => $name,)+
                    }
                }

                /// A notice name, which will be applied as the label on the
                /// metric that is counting notices labelled by notice kind.
                pub fn metric_label(&self) -> &str {
                    match self {
                        $(Self::$ty => stringify!($ty),)+
                    }
                }
            }

            $(
                impl From<$ty> for RawOptimizerNotice {
                    fn from(value: $ty) -> Self {
                        RawOptimizerNotice::$ty(value)
                    }
                }
            )+
        }
    };
}

raw_optimizer_notices![
    IndexTooWideForLiteralConstraints => "Index too wide for literal constraints",
    IndexKeyEmpty => "Empty index key",
];

impl RawOptimizerNotice {
    /// Turns a vector of notices into a vector of strings that can be used in
    /// EXPLAIN.
    ///
    /// This method should be consistent with [`OptimizerNotice::explain`].
    pub fn explain(
        notices: &Vec<RawOptimizerNotice>,
        humanizer: &dyn ExprHumanizer,
        redacted: bool,
    ) -> Result<Vec<String>, Error> {
        let mut notice_strings = Vec::new();
        for notice in notices {
            if notice.is_valid(humanizer) {
                let mut s = String::new();
                write!(s, "  - Notice: {}\n", notice.message(humanizer, redacted))?;
                write!(s, "    Hint: {}", notice.hint(humanizer, redacted))?;
                notice_strings.push(s);
            }
        }
        Ok(notice_strings)
    }

    /// Returns `true` iff all dependencies for this notice still exist.
    ///
    /// This method should be consistent with [`OptimizerNotice::is_valid`].
    fn is_valid(&self, humanizer: &dyn ExprHumanizer) -> bool {
        self.dependencies()
            .iter()
            .all(|id| humanizer.id_exists(*id))
    }

    /// A notice name, which will be applied as the label on the metric that is
    /// counting notices labelled by notice type.
    pub fn metric_label(&self) -> &str {
        OptimizerNoticeKind::from(self).as_str()
    }
}
