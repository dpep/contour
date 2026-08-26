//! VENDORED from rwr `src/pattern/generated.rs` (~/code/lib/rust/rwr, MIT,
//! same author), GENERATED there by `script/gen-compare.py` -- do not edit
//! by hand. Regenerate upstream and re-copy; the only local change is the
//! `Atom` import path.
//!
//! Child and atom extraction for every Prism node (D36). Equality is
//! variant + atoms + children; location fields are deliberately absent
//! here, because spelling is opt-in through `where:` predicates rather
//! than baked into node equality.

// `locations` and `dup` serve rwr's rewriter; contour folds, so it needs
// neither. Kept so a sync stays a re-copy.
#![allow(dead_code)]

use super::norm::Atom;
use ruby_prism::Node;

/// Immediate child nodes, in field order.
#[allow(clippy::too_many_lines, unused_variables)]
pub(crate) fn children<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    let mut out = Vec::new();
    match node {
        Node::AliasGlobalVariableNode { .. } => {
            let Some(n) = node.as_alias_global_variable_node() else {
                return out;
            };
            out.push(n.new_name());
            out.push(n.old_name());
        }
        Node::AliasMethodNode { .. } => {
            let Some(n) = node.as_alias_method_node() else {
                return out;
            };
            out.push(n.new_name());
            out.push(n.old_name());
        }
        Node::AlternationPatternNode { .. } => {
            let Some(n) = node.as_alternation_pattern_node() else {
                return out;
            };
            out.push(n.left());
            out.push(n.right());
        }
        Node::AndNode { .. } => {
            let Some(n) = node.as_and_node() else {
                return out;
            };
            out.push(n.left());
            out.push(n.right());
        }
        Node::ArgumentsNode { .. } => {
            let Some(n) = node.as_arguments_node() else {
                return out;
            };
            out.extend(n.arguments().iter());
        }
        Node::ArrayNode { .. } => {
            let Some(n) = node.as_array_node() else {
                return out;
            };
            out.extend(n.elements().iter());
        }
        Node::ArrayPatternNode { .. } => {
            let Some(n) = node.as_array_pattern_node() else {
                return out;
            };
            if let Some(x) = n.constant() {
                out.push(x);
            }
            out.extend(n.requireds().iter());
            if let Some(x) = n.rest() {
                out.push(x);
            }
            out.extend(n.posts().iter());
        }
        Node::AssocNode { .. } => {
            let Some(n) = node.as_assoc_node() else {
                return out;
            };
            out.push(n.key());
            out.push(n.value());
        }
        Node::AssocSplatNode { .. } => {
            let Some(n) = node.as_assoc_splat_node() else {
                return out;
            };
            if let Some(x) = n.value() {
                out.push(x);
            }
        }
        Node::BeginNode { .. } => {
            let Some(n) = node.as_begin_node() else {
                return out;
            };
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
            if let Some(x) = n.rescue_clause() {
                out.push(x.as_node());
            }
            if let Some(x) = n.else_clause() {
                out.push(x.as_node());
            }
            if let Some(x) = n.ensure_clause() {
                out.push(x.as_node());
            }
        }
        Node::BlockArgumentNode { .. } => {
            let Some(n) = node.as_block_argument_node() else {
                return out;
            };
            if let Some(x) = n.expression() {
                out.push(x);
            }
        }
        Node::BlockNode { .. } => {
            let Some(n) = node.as_block_node() else {
                return out;
            };
            if let Some(x) = n.parameters() {
                out.push(x);
            }
            if let Some(x) = n.body() {
                out.push(x);
            }
        }
        Node::BlockParametersNode { .. } => {
            let Some(n) = node.as_block_parameters_node() else {
                return out;
            };
            if let Some(x) = n.parameters() {
                out.push(x.as_node());
            }
            out.extend(n.locals().iter());
        }
        Node::BreakNode { .. } => {
            let Some(n) = node.as_break_node() else {
                return out;
            };
            if let Some(x) = n.arguments() {
                out.push(x.as_node());
            }
        }
        Node::CallAndWriteNode { .. } => {
            let Some(n) = node.as_call_and_write_node() else {
                return out;
            };
            if let Some(x) = n.receiver() {
                out.push(x);
            }
            out.push(n.value());
        }
        Node::CallNode { .. } => {
            let Some(n) = node.as_call_node() else {
                return out;
            };
            if let Some(x) = n.receiver() {
                out.push(x);
            }
            if let Some(x) = n.arguments() {
                out.push(x.as_node());
            }
            if let Some(x) = n.block() {
                out.push(x);
            }
        }
        Node::CallOperatorWriteNode { .. } => {
            let Some(n) = node.as_call_operator_write_node() else {
                return out;
            };
            if let Some(x) = n.receiver() {
                out.push(x);
            }
            out.push(n.value());
        }
        Node::CallOrWriteNode { .. } => {
            let Some(n) = node.as_call_or_write_node() else {
                return out;
            };
            if let Some(x) = n.receiver() {
                out.push(x);
            }
            out.push(n.value());
        }
        Node::CallTargetNode { .. } => {
            let Some(n) = node.as_call_target_node() else {
                return out;
            };
            out.push(n.receiver());
        }
        Node::CapturePatternNode { .. } => {
            let Some(n) = node.as_capture_pattern_node() else {
                return out;
            };
            out.push(n.value());
            out.push(n.target().as_node());
        }
        Node::CaseMatchNode { .. } => {
            let Some(n) = node.as_case_match_node() else {
                return out;
            };
            if let Some(x) = n.predicate() {
                out.push(x);
            }
            out.extend(n.conditions().iter());
            if let Some(x) = n.else_clause() {
                out.push(x.as_node());
            }
        }
        Node::CaseNode { .. } => {
            let Some(n) = node.as_case_node() else {
                return out;
            };
            if let Some(x) = n.predicate() {
                out.push(x);
            }
            out.extend(n.conditions().iter());
            if let Some(x) = n.else_clause() {
                out.push(x.as_node());
            }
        }
        Node::ClassNode { .. } => {
            let Some(n) = node.as_class_node() else {
                return out;
            };
            out.push(n.constant_path());
            if let Some(x) = n.superclass() {
                out.push(x);
            }
            if let Some(x) = n.body() {
                out.push(x);
            }
        }
        Node::ClassVariableAndWriteNode { .. } => {
            let Some(n) = node.as_class_variable_and_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::ClassVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_class_variable_operator_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::ClassVariableOrWriteNode { .. } => {
            let Some(n) = node.as_class_variable_or_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::ClassVariableWriteNode { .. } => {
            let Some(n) = node.as_class_variable_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::ConstantAndWriteNode { .. } => {
            let Some(n) = node.as_constant_and_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::ConstantOperatorWriteNode { .. } => {
            let Some(n) = node.as_constant_operator_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::ConstantOrWriteNode { .. } => {
            let Some(n) = node.as_constant_or_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::ConstantPathAndWriteNode { .. } => {
            let Some(n) = node.as_constant_path_and_write_node() else {
                return out;
            };
            out.push(n.target().as_node());
            out.push(n.value());
        }
        Node::ConstantPathNode { .. } => {
            let Some(n) = node.as_constant_path_node() else {
                return out;
            };
            if let Some(x) = n.parent() {
                out.push(x);
            }
        }
        Node::ConstantPathOperatorWriteNode { .. } => {
            let Some(n) = node.as_constant_path_operator_write_node() else {
                return out;
            };
            out.push(n.target().as_node());
            out.push(n.value());
        }
        Node::ConstantPathOrWriteNode { .. } => {
            let Some(n) = node.as_constant_path_or_write_node() else {
                return out;
            };
            out.push(n.target().as_node());
            out.push(n.value());
        }
        Node::ConstantPathTargetNode { .. } => {
            let Some(n) = node.as_constant_path_target_node() else {
                return out;
            };
            if let Some(x) = n.parent() {
                out.push(x);
            }
        }
        Node::ConstantPathWriteNode { .. } => {
            let Some(n) = node.as_constant_path_write_node() else {
                return out;
            };
            out.push(n.target().as_node());
            out.push(n.value());
        }
        Node::ConstantWriteNode { .. } => {
            let Some(n) = node.as_constant_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::DefNode { .. } => {
            let Some(n) = node.as_def_node() else {
                return out;
            };
            if let Some(x) = n.receiver() {
                out.push(x);
            }
            if let Some(x) = n.parameters() {
                out.push(x.as_node());
            }
            if let Some(x) = n.body() {
                out.push(x);
            }
        }
        Node::DefinedNode { .. } => {
            let Some(n) = node.as_defined_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::ElseNode { .. } => {
            let Some(n) = node.as_else_node() else {
                return out;
            };
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
        }
        Node::EmbeddedStatementsNode { .. } => {
            let Some(n) = node.as_embedded_statements_node() else {
                return out;
            };
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
        }
        Node::EmbeddedVariableNode { .. } => {
            let Some(n) = node.as_embedded_variable_node() else {
                return out;
            };
            out.push(n.variable());
        }
        Node::EnsureNode { .. } => {
            let Some(n) = node.as_ensure_node() else {
                return out;
            };
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
        }
        Node::FindPatternNode { .. } => {
            let Some(n) = node.as_find_pattern_node() else {
                return out;
            };
            if let Some(x) = n.constant() {
                out.push(x);
            }
            out.push(n.left().as_node());
            out.extend(n.requireds().iter());
            out.push(n.right());
        }
        Node::FlipFlopNode { .. } => {
            let Some(n) = node.as_flip_flop_node() else {
                return out;
            };
            if let Some(x) = n.left() {
                out.push(x);
            }
            if let Some(x) = n.right() {
                out.push(x);
            }
        }
        Node::ForNode { .. } => {
            let Some(n) = node.as_for_node() else {
                return out;
            };
            out.push(n.index());
            out.push(n.collection());
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
        }
        Node::ForwardingSuperNode { .. } => {
            let Some(n) = node.as_forwarding_super_node() else {
                return out;
            };
            if let Some(x) = n.block() {
                out.push(x.as_node());
            }
        }
        Node::GlobalVariableAndWriteNode { .. } => {
            let Some(n) = node.as_global_variable_and_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::GlobalVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_global_variable_operator_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::GlobalVariableOrWriteNode { .. } => {
            let Some(n) = node.as_global_variable_or_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::GlobalVariableWriteNode { .. } => {
            let Some(n) = node.as_global_variable_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::HashNode { .. } => {
            let Some(n) = node.as_hash_node() else {
                return out;
            };
            out.extend(n.elements().iter());
        }
        Node::HashPatternNode { .. } => {
            let Some(n) = node.as_hash_pattern_node() else {
                return out;
            };
            if let Some(x) = n.constant() {
                out.push(x);
            }
            out.extend(n.elements().iter());
            if let Some(x) = n.rest() {
                out.push(x);
            }
        }
        Node::IfNode { .. } => {
            let Some(n) = node.as_if_node() else {
                return out;
            };
            out.push(n.predicate());
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
            if let Some(x) = n.subsequent() {
                out.push(x);
            }
        }
        Node::ImaginaryNode { .. } => {
            let Some(n) = node.as_imaginary_node() else {
                return out;
            };
            out.push(n.numeric());
        }
        Node::ImplicitNode { .. } => {
            let Some(n) = node.as_implicit_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::InNode { .. } => {
            let Some(n) = node.as_in_node() else {
                return out;
            };
            out.push(n.pattern());
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
        }
        Node::IndexAndWriteNode { .. } => {
            let Some(n) = node.as_index_and_write_node() else {
                return out;
            };
            if let Some(x) = n.receiver() {
                out.push(x);
            }
            if let Some(x) = n.arguments() {
                out.push(x.as_node());
            }
            if let Some(x) = n.block() {
                out.push(x.as_node());
            }
            out.push(n.value());
        }
        Node::IndexOperatorWriteNode { .. } => {
            let Some(n) = node.as_index_operator_write_node() else {
                return out;
            };
            if let Some(x) = n.receiver() {
                out.push(x);
            }
            if let Some(x) = n.arguments() {
                out.push(x.as_node());
            }
            if let Some(x) = n.block() {
                out.push(x.as_node());
            }
            out.push(n.value());
        }
        Node::IndexOrWriteNode { .. } => {
            let Some(n) = node.as_index_or_write_node() else {
                return out;
            };
            if let Some(x) = n.receiver() {
                out.push(x);
            }
            if let Some(x) = n.arguments() {
                out.push(x.as_node());
            }
            if let Some(x) = n.block() {
                out.push(x.as_node());
            }
            out.push(n.value());
        }
        Node::IndexTargetNode { .. } => {
            let Some(n) = node.as_index_target_node() else {
                return out;
            };
            out.push(n.receiver());
            if let Some(x) = n.arguments() {
                out.push(x.as_node());
            }
            if let Some(x) = n.block() {
                out.push(x.as_node());
            }
        }
        Node::InstanceVariableAndWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_and_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::InstanceVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_operator_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::InstanceVariableOrWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_or_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::InstanceVariableWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::InterpolatedMatchLastLineNode { .. } => {
            let Some(n) = node.as_interpolated_match_last_line_node() else {
                return out;
            };
            out.extend(n.parts().iter());
        }
        Node::InterpolatedRegularExpressionNode { .. } => {
            let Some(n) = node.as_interpolated_regular_expression_node() else {
                return out;
            };
            out.extend(n.parts().iter());
        }
        Node::InterpolatedStringNode { .. } => {
            let Some(n) = node.as_interpolated_string_node() else {
                return out;
            };
            out.extend(n.parts().iter());
        }
        Node::InterpolatedSymbolNode { .. } => {
            let Some(n) = node.as_interpolated_symbol_node() else {
                return out;
            };
            out.extend(n.parts().iter());
        }
        Node::InterpolatedXStringNode { .. } => {
            let Some(n) = node.as_interpolated_x_string_node() else {
                return out;
            };
            out.extend(n.parts().iter());
        }
        Node::KeywordHashNode { .. } => {
            let Some(n) = node.as_keyword_hash_node() else {
                return out;
            };
            out.extend(n.elements().iter());
        }
        Node::LambdaNode { .. } => {
            let Some(n) = node.as_lambda_node() else {
                return out;
            };
            if let Some(x) = n.parameters() {
                out.push(x);
            }
            if let Some(x) = n.body() {
                out.push(x);
            }
        }
        Node::LocalVariableAndWriteNode { .. } => {
            let Some(n) = node.as_local_variable_and_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::LocalVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_local_variable_operator_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::LocalVariableOrWriteNode { .. } => {
            let Some(n) = node.as_local_variable_or_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::LocalVariableWriteNode { .. } => {
            let Some(n) = node.as_local_variable_write_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::MatchPredicateNode { .. } => {
            let Some(n) = node.as_match_predicate_node() else {
                return out;
            };
            out.push(n.value());
            out.push(n.pattern());
        }
        Node::MatchRequiredNode { .. } => {
            let Some(n) = node.as_match_required_node() else {
                return out;
            };
            out.push(n.value());
            out.push(n.pattern());
        }
        Node::MatchWriteNode { .. } => {
            let Some(n) = node.as_match_write_node() else {
                return out;
            };
            out.push(n.call().as_node());
            out.extend(n.targets().iter());
        }
        Node::ModuleNode { .. } => {
            let Some(n) = node.as_module_node() else {
                return out;
            };
            out.push(n.constant_path());
            if let Some(x) = n.body() {
                out.push(x);
            }
        }
        Node::MultiTargetNode { .. } => {
            let Some(n) = node.as_multi_target_node() else {
                return out;
            };
            out.extend(n.lefts().iter());
            if let Some(x) = n.rest() {
                out.push(x);
            }
            out.extend(n.rights().iter());
        }
        Node::MultiWriteNode { .. } => {
            let Some(n) = node.as_multi_write_node() else {
                return out;
            };
            out.extend(n.lefts().iter());
            if let Some(x) = n.rest() {
                out.push(x);
            }
            out.extend(n.rights().iter());
            out.push(n.value());
        }
        Node::NextNode { .. } => {
            let Some(n) = node.as_next_node() else {
                return out;
            };
            if let Some(x) = n.arguments() {
                out.push(x.as_node());
            }
        }
        Node::OptionalKeywordParameterNode { .. } => {
            let Some(n) = node.as_optional_keyword_parameter_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::OptionalParameterNode { .. } => {
            let Some(n) = node.as_optional_parameter_node() else {
                return out;
            };
            out.push(n.value());
        }
        Node::OrNode { .. } => {
            let Some(n) = node.as_or_node() else {
                return out;
            };
            out.push(n.left());
            out.push(n.right());
        }
        Node::ParametersNode { .. } => {
            let Some(n) = node.as_parameters_node() else {
                return out;
            };
            out.extend(n.requireds().iter());
            out.extend(n.optionals().iter());
            if let Some(x) = n.rest() {
                out.push(x);
            }
            out.extend(n.posts().iter());
            out.extend(n.keywords().iter());
            if let Some(x) = n.keyword_rest() {
                out.push(x);
            }
            if let Some(x) = n.block() {
                out.push(x.as_node());
            }
        }
        Node::ParenthesesNode { .. } => {
            let Some(n) = node.as_parentheses_node() else {
                return out;
            };
            if let Some(x) = n.body() {
                out.push(x);
            }
        }
        Node::PinnedExpressionNode { .. } => {
            let Some(n) = node.as_pinned_expression_node() else {
                return out;
            };
            out.push(n.expression());
        }
        Node::PinnedVariableNode { .. } => {
            let Some(n) = node.as_pinned_variable_node() else {
                return out;
            };
            out.push(n.variable());
        }
        Node::PostExecutionNode { .. } => {
            let Some(n) = node.as_post_execution_node() else {
                return out;
            };
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
        }
        Node::PreExecutionNode { .. } => {
            let Some(n) = node.as_pre_execution_node() else {
                return out;
            };
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
        }
        Node::ProgramNode { .. } => {
            let Some(n) = node.as_program_node() else {
                return out;
            };
            out.push(n.statements().as_node());
        }
        Node::RangeNode { .. } => {
            let Some(n) = node.as_range_node() else {
                return out;
            };
            if let Some(x) = n.left() {
                out.push(x);
            }
            if let Some(x) = n.right() {
                out.push(x);
            }
        }
        Node::RescueModifierNode { .. } => {
            let Some(n) = node.as_rescue_modifier_node() else {
                return out;
            };
            out.push(n.expression());
            out.push(n.rescue_expression());
        }
        Node::RescueNode { .. } => {
            let Some(n) = node.as_rescue_node() else {
                return out;
            };
            out.extend(n.exceptions().iter());
            if let Some(x) = n.reference() {
                out.push(x);
            }
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
            if let Some(x) = n.subsequent() {
                out.push(x.as_node());
            }
        }
        Node::ReturnNode { .. } => {
            let Some(n) = node.as_return_node() else {
                return out;
            };
            if let Some(x) = n.arguments() {
                out.push(x.as_node());
            }
        }
        Node::ShareableConstantNode { .. } => {
            let Some(n) = node.as_shareable_constant_node() else {
                return out;
            };
            out.push(n.write());
        }
        Node::SingletonClassNode { .. } => {
            let Some(n) = node.as_singleton_class_node() else {
                return out;
            };
            out.push(n.expression());
            if let Some(x) = n.body() {
                out.push(x);
            }
        }
        Node::SplatNode { .. } => {
            let Some(n) = node.as_splat_node() else {
                return out;
            };
            if let Some(x) = n.expression() {
                out.push(x);
            }
        }
        Node::StatementsNode { .. } => {
            let Some(n) = node.as_statements_node() else {
                return out;
            };
            out.extend(n.body().iter());
        }
        Node::SuperNode { .. } => {
            let Some(n) = node.as_super_node() else {
                return out;
            };
            if let Some(x) = n.arguments() {
                out.push(x.as_node());
            }
            if let Some(x) = n.block() {
                out.push(x);
            }
        }
        Node::UndefNode { .. } => {
            let Some(n) = node.as_undef_node() else {
                return out;
            };
            out.extend(n.names().iter());
        }
        Node::UnlessNode { .. } => {
            let Some(n) = node.as_unless_node() else {
                return out;
            };
            out.push(n.predicate());
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
            if let Some(x) = n.else_clause() {
                out.push(x.as_node());
            }
        }
        Node::UntilNode { .. } => {
            let Some(n) = node.as_until_node() else {
                return out;
            };
            out.push(n.predicate());
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
        }
        Node::WhenNode { .. } => {
            let Some(n) = node.as_when_node() else {
                return out;
            };
            out.extend(n.conditions().iter());
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
        }
        Node::WhileNode { .. } => {
            let Some(n) = node.as_while_node() else {
                return out;
            };
            out.push(n.predicate());
            if let Some(x) = n.statements() {
                out.push(x.as_node());
            }
        }
        Node::YieldNode { .. } => {
            let Some(n) = node.as_yield_node() else {
                return out;
            };
            if let Some(x) = n.arguments() {
                out.push(x.as_node());
            }
        }
        _ => {}
    }
    out
}

/// Every location a node carries, including the trailing ones -- a heredoc's
/// `closing_loc` sits past its body, which is what lets `effective_range`
/// cover content the node's own location excludes (D14).
#[allow(clippy::too_many_lines)]
pub(crate) fn locations<'pr>(node: &Node<'pr>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    match node {
        Node::AliasGlobalVariableNode { .. } => {
            let Some(n) = node.as_alias_global_variable_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
        }
        Node::AliasMethodNode { .. } => {
            let Some(n) = node.as_alias_method_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
        }
        Node::AlternationPatternNode { .. } => {
            let Some(n) = node.as_alternation_pattern_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::AndNode { .. } => {
            let Some(n) = node.as_and_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ArrayNode { .. } => {
            let Some(n) = node.as_array_node() else {
                return out;
            };
            if let Some(l) = n.opening_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::ArrayPatternNode { .. } => {
            let Some(n) = node.as_array_pattern_node() else {
                return out;
            };
            if let Some(l) = n.opening_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::AssocNode { .. } => {
            let Some(n) = node.as_assoc_node() else {
                return out;
            };
            if let Some(l) = n.operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::AssocSplatNode { .. } => {
            let Some(n) = node.as_assoc_splat_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::BeginNode { .. } => {
            let Some(n) = node.as_begin_node() else {
                return out;
            };
            if let Some(l) = n.begin_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.end_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::BlockArgumentNode { .. } => {
            let Some(n) = node.as_block_argument_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::BlockNode { .. } => {
            let Some(n) = node.as_block_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::BlockParameterNode { .. } => {
            let Some(n) = node.as_block_parameter_node() else {
                return out;
            };
            if let Some(l) = n.name_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::BlockParametersNode { .. } => {
            let Some(n) = node.as_block_parameters_node() else {
                return out;
            };
            if let Some(l) = n.opening_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::BreakNode { .. } => {
            let Some(n) = node.as_break_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
        }
        Node::CallAndWriteNode { .. } => {
            let Some(n) = node.as_call_and_write_node() else {
                return out;
            };
            if let Some(l) = n.call_operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.message_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::CallNode { .. } => {
            let Some(n) = node.as_call_node() else {
                return out;
            };
            if let Some(l) = n.call_operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.message_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.opening_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.equal_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::CallOperatorWriteNode { .. } => {
            let Some(n) = node.as_call_operator_write_node() else {
                return out;
            };
            if let Some(l) = n.call_operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.message_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((
                n.binary_operator_loc().start_offset(),
                n.binary_operator_loc().end_offset(),
            ));
        }
        Node::CallOrWriteNode { .. } => {
            let Some(n) = node.as_call_or_write_node() else {
                return out;
            };
            if let Some(l) = n.call_operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.message_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::CallTargetNode { .. } => {
            let Some(n) = node.as_call_target_node() else {
                return out;
            };
            out.push((
                n.call_operator_loc().start_offset(),
                n.call_operator_loc().end_offset(),
            ));
            out.push((n.message_loc().start_offset(), n.message_loc().end_offset()));
        }
        Node::CapturePatternNode { .. } => {
            let Some(n) = node.as_capture_pattern_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::CaseMatchNode { .. } => {
            let Some(n) = node.as_case_match_node() else {
                return out;
            };
            out.push((
                n.case_keyword_loc().start_offset(),
                n.case_keyword_loc().end_offset(),
            ));
            out.push((
                n.end_keyword_loc().start_offset(),
                n.end_keyword_loc().end_offset(),
            ));
        }
        Node::CaseNode { .. } => {
            let Some(n) = node.as_case_node() else {
                return out;
            };
            out.push((
                n.case_keyword_loc().start_offset(),
                n.case_keyword_loc().end_offset(),
            ));
            out.push((
                n.end_keyword_loc().start_offset(),
                n.end_keyword_loc().end_offset(),
            ));
        }
        Node::ClassNode { .. } => {
            let Some(n) = node.as_class_node() else {
                return out;
            };
            out.push((
                n.class_keyword_loc().start_offset(),
                n.class_keyword_loc().end_offset(),
            ));
            if let Some(l) = n.inheritance_operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((
                n.end_keyword_loc().start_offset(),
                n.end_keyword_loc().end_offset(),
            ));
        }
        Node::ClassVariableAndWriteNode { .. } => {
            let Some(n) = node.as_class_variable_and_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ClassVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_class_variable_operator_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.binary_operator_loc().start_offset(),
                n.binary_operator_loc().end_offset(),
            ));
        }
        Node::ClassVariableOrWriteNode { .. } => {
            let Some(n) = node.as_class_variable_or_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ClassVariableWriteNode { .. } => {
            let Some(n) = node.as_class_variable_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ConstantAndWriteNode { .. } => {
            let Some(n) = node.as_constant_and_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ConstantOperatorWriteNode { .. } => {
            let Some(n) = node.as_constant_operator_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.binary_operator_loc().start_offset(),
                n.binary_operator_loc().end_offset(),
            ));
        }
        Node::ConstantOrWriteNode { .. } => {
            let Some(n) = node.as_constant_or_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ConstantPathAndWriteNode { .. } => {
            let Some(n) = node.as_constant_path_and_write_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ConstantPathNode { .. } => {
            let Some(n) = node.as_constant_path_node() else {
                return out;
            };
            out.push((
                n.delimiter_loc().start_offset(),
                n.delimiter_loc().end_offset(),
            ));
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
        }
        Node::ConstantPathOperatorWriteNode { .. } => {
            let Some(n) = node.as_constant_path_operator_write_node() else {
                return out;
            };
            out.push((
                n.binary_operator_loc().start_offset(),
                n.binary_operator_loc().end_offset(),
            ));
        }
        Node::ConstantPathOrWriteNode { .. } => {
            let Some(n) = node.as_constant_path_or_write_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ConstantPathTargetNode { .. } => {
            let Some(n) = node.as_constant_path_target_node() else {
                return out;
            };
            out.push((
                n.delimiter_loc().start_offset(),
                n.delimiter_loc().end_offset(),
            ));
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
        }
        Node::ConstantPathWriteNode { .. } => {
            let Some(n) = node.as_constant_path_write_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ConstantWriteNode { .. } => {
            let Some(n) = node.as_constant_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::DefNode { .. } => {
            let Some(n) = node.as_def_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.def_keyword_loc().start_offset(),
                n.def_keyword_loc().end_offset(),
            ));
            if let Some(l) = n.operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.lparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.rparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.equal_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.end_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::DefinedNode { .. } => {
            let Some(n) = node.as_defined_node() else {
                return out;
            };
            if let Some(l) = n.lparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.rparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
        }
        Node::ElseNode { .. } => {
            let Some(n) = node.as_else_node() else {
                return out;
            };
            out.push((
                n.else_keyword_loc().start_offset(),
                n.else_keyword_loc().end_offset(),
            ));
            if let Some(l) = n.end_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::EmbeddedStatementsNode { .. } => {
            let Some(n) = node.as_embedded_statements_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::EmbeddedVariableNode { .. } => {
            let Some(n) = node.as_embedded_variable_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::EnsureNode { .. } => {
            let Some(n) = node.as_ensure_node() else {
                return out;
            };
            out.push((
                n.ensure_keyword_loc().start_offset(),
                n.ensure_keyword_loc().end_offset(),
            ));
            out.push((
                n.end_keyword_loc().start_offset(),
                n.end_keyword_loc().end_offset(),
            ));
        }
        Node::FindPatternNode { .. } => {
            let Some(n) = node.as_find_pattern_node() else {
                return out;
            };
            if let Some(l) = n.opening_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::FlipFlopNode { .. } => {
            let Some(n) = node.as_flip_flop_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ForNode { .. } => {
            let Some(n) = node.as_for_node() else {
                return out;
            };
            out.push((
                n.for_keyword_loc().start_offset(),
                n.for_keyword_loc().end_offset(),
            ));
            out.push((
                n.in_keyword_loc().start_offset(),
                n.in_keyword_loc().end_offset(),
            ));
            if let Some(l) = n.do_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((
                n.end_keyword_loc().start_offset(),
                n.end_keyword_loc().end_offset(),
            ));
        }
        Node::GlobalVariableAndWriteNode { .. } => {
            let Some(n) = node.as_global_variable_and_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::GlobalVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_global_variable_operator_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.binary_operator_loc().start_offset(),
                n.binary_operator_loc().end_offset(),
            ));
        }
        Node::GlobalVariableOrWriteNode { .. } => {
            let Some(n) = node.as_global_variable_or_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::GlobalVariableWriteNode { .. } => {
            let Some(n) = node.as_global_variable_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::HashNode { .. } => {
            let Some(n) = node.as_hash_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::HashPatternNode { .. } => {
            let Some(n) = node.as_hash_pattern_node() else {
                return out;
            };
            if let Some(l) = n.opening_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::IfNode { .. } => {
            let Some(n) = node.as_if_node() else {
                return out;
            };
            if let Some(l) = n.if_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.then_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.end_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::InNode { .. } => {
            let Some(n) = node.as_in_node() else {
                return out;
            };
            out.push((n.in_loc().start_offset(), n.in_loc().end_offset()));
            if let Some(l) = n.then_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::IndexAndWriteNode { .. } => {
            let Some(n) = node.as_index_and_write_node() else {
                return out;
            };
            if let Some(l) = n.call_operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::IndexOperatorWriteNode { .. } => {
            let Some(n) = node.as_index_operator_write_node() else {
                return out;
            };
            if let Some(l) = n.call_operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
            out.push((
                n.binary_operator_loc().start_offset(),
                n.binary_operator_loc().end_offset(),
            ));
        }
        Node::IndexOrWriteNode { .. } => {
            let Some(n) = node.as_index_or_write_node() else {
                return out;
            };
            if let Some(l) = n.call_operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::IndexTargetNode { .. } => {
            let Some(n) = node.as_index_target_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::InstanceVariableAndWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_and_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::InstanceVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_operator_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.binary_operator_loc().start_offset(),
                n.binary_operator_loc().end_offset(),
            ));
        }
        Node::InstanceVariableOrWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_or_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::InstanceVariableWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::InterpolatedMatchLastLineNode { .. } => {
            let Some(n) = node.as_interpolated_match_last_line_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::InterpolatedRegularExpressionNode { .. } => {
            let Some(n) = node.as_interpolated_regular_expression_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::InterpolatedStringNode { .. } => {
            let Some(n) = node.as_interpolated_string_node() else {
                return out;
            };
            if let Some(l) = n.opening_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::InterpolatedSymbolNode { .. } => {
            let Some(n) = node.as_interpolated_symbol_node() else {
                return out;
            };
            if let Some(l) = n.opening_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::InterpolatedXStringNode { .. } => {
            let Some(n) = node.as_interpolated_x_string_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::KeywordRestParameterNode { .. } => {
            let Some(n) = node.as_keyword_rest_parameter_node() else {
                return out;
            };
            if let Some(l) = n.name_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::LambdaNode { .. } => {
            let Some(n) = node.as_lambda_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::LocalVariableAndWriteNode { .. } => {
            let Some(n) = node.as_local_variable_and_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::LocalVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_local_variable_operator_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.binary_operator_loc().start_offset(),
                n.binary_operator_loc().end_offset(),
            ));
        }
        Node::LocalVariableOrWriteNode { .. } => {
            let Some(n) = node.as_local_variable_or_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::LocalVariableWriteNode { .. } => {
            let Some(n) = node.as_local_variable_write_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::MatchLastLineNode { .. } => {
            let Some(n) = node.as_match_last_line_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.content_loc().start_offset(), n.content_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::MatchPredicateNode { .. } => {
            let Some(n) = node.as_match_predicate_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::MatchRequiredNode { .. } => {
            let Some(n) = node.as_match_required_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ModuleNode { .. } => {
            let Some(n) = node.as_module_node() else {
                return out;
            };
            out.push((
                n.module_keyword_loc().start_offset(),
                n.module_keyword_loc().end_offset(),
            ));
            out.push((
                n.end_keyword_loc().start_offset(),
                n.end_keyword_loc().end_offset(),
            ));
        }
        Node::MultiTargetNode { .. } => {
            let Some(n) = node.as_multi_target_node() else {
                return out;
            };
            if let Some(l) = n.lparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.rparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::MultiWriteNode { .. } => {
            let Some(n) = node.as_multi_write_node() else {
                return out;
            };
            if let Some(l) = n.lparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.rparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::NextNode { .. } => {
            let Some(n) = node.as_next_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
        }
        Node::NoKeywordsParameterNode { .. } => {
            let Some(n) = node.as_no_keywords_parameter_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
        }
        Node::OptionalKeywordParameterNode { .. } => {
            let Some(n) = node.as_optional_keyword_parameter_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
        }
        Node::OptionalParameterNode { .. } => {
            let Some(n) = node.as_optional_parameter_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::OrNode { .. } => {
            let Some(n) = node.as_or_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ParenthesesNode { .. } => {
            let Some(n) = node.as_parentheses_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::PinnedExpressionNode { .. } => {
            let Some(n) = node.as_pinned_expression_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
            out.push((n.lparen_loc().start_offset(), n.lparen_loc().end_offset()));
            out.push((n.rparen_loc().start_offset(), n.rparen_loc().end_offset()));
        }
        Node::PinnedVariableNode { .. } => {
            let Some(n) = node.as_pinned_variable_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::PostExecutionNode { .. } => {
            let Some(n) = node.as_post_execution_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::PreExecutionNode { .. } => {
            let Some(n) = node.as_pre_execution_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::RangeNode { .. } => {
            let Some(n) = node.as_range_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::RegularExpressionNode { .. } => {
            let Some(n) = node.as_regular_expression_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.content_loc().start_offset(), n.content_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::RequiredKeywordParameterNode { .. } => {
            let Some(n) = node.as_required_keyword_parameter_node() else {
                return out;
            };
            out.push((n.name_loc().start_offset(), n.name_loc().end_offset()));
        }
        Node::RescueModifierNode { .. } => {
            let Some(n) = node.as_rescue_modifier_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
        }
        Node::RescueNode { .. } => {
            let Some(n) = node.as_rescue_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
            if let Some(l) = n.operator_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.then_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::RestParameterNode { .. } => {
            let Some(n) = node.as_rest_parameter_node() else {
                return out;
            };
            if let Some(l) = n.name_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::ReturnNode { .. } => {
            let Some(n) = node.as_return_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
        }
        Node::SingletonClassNode { .. } => {
            let Some(n) = node.as_singleton_class_node() else {
                return out;
            };
            out.push((
                n.class_keyword_loc().start_offset(),
                n.class_keyword_loc().end_offset(),
            ));
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
            out.push((
                n.end_keyword_loc().start_offset(),
                n.end_keyword_loc().end_offset(),
            ));
        }
        Node::SplatNode { .. } => {
            let Some(n) = node.as_splat_node() else {
                return out;
            };
            out.push((
                n.operator_loc().start_offset(),
                n.operator_loc().end_offset(),
            ));
        }
        Node::StringNode { .. } => {
            let Some(n) = node.as_string_node() else {
                return out;
            };
            if let Some(l) = n.opening_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            out.push((n.content_loc().start_offset(), n.content_loc().end_offset()));
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::SuperNode { .. } => {
            let Some(n) = node.as_super_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
            if let Some(l) = n.lparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.rparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::SymbolNode { .. } => {
            let Some(n) = node.as_symbol_node() else {
                return out;
            };
            if let Some(l) = n.opening_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.value_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::UndefNode { .. } => {
            let Some(n) = node.as_undef_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
        }
        Node::UnlessNode { .. } => {
            let Some(n) = node.as_unless_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
            if let Some(l) = n.then_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.end_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::UntilNode { .. } => {
            let Some(n) = node.as_until_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
            if let Some(l) = n.do_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::WhenNode { .. } => {
            let Some(n) = node.as_when_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
            if let Some(l) = n.then_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::WhileNode { .. } => {
            let Some(n) = node.as_while_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
            if let Some(l) = n.do_keyword_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.closing_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        Node::XStringNode { .. } => {
            let Some(n) = node.as_x_string_node() else {
                return out;
            };
            out.push((n.opening_loc().start_offset(), n.opening_loc().end_offset()));
            out.push((n.content_loc().start_offset(), n.content_loc().end_offset()));
            out.push((n.closing_loc().start_offset(), n.closing_loc().end_offset()));
        }
        Node::YieldNode { .. } => {
            let Some(n) = node.as_yield_node() else {
                return out;
            };
            out.push((n.keyword_loc().start_offset(), n.keyword_loc().end_offset()));
            if let Some(l) = n.lparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
            if let Some(l) = n.rparen_loc() {
                out.push((l.start_offset(), l.end_offset()));
            }
        }
        _ => {}
    }
    out
}

/// Duplicate a node handle.
///
/// `Node` holds raw pointers and derives neither `Clone` nor `Copy`, but a
/// handle is just those pointers -- copying one is free and does not
/// duplicate the underlying tree, which the parse result still owns.
#[allow(clippy::too_many_lines)]
pub(crate) fn dup<'pr>(node: &Node<'pr>) -> Node<'pr> {
    match node {
        Node::AliasGlobalVariableNode {
            parser,
            pointer,
            marker,
        } => Node::AliasGlobalVariableNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::AliasMethodNode {
            parser,
            pointer,
            marker,
        } => Node::AliasMethodNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::AlternationPatternNode {
            parser,
            pointer,
            marker,
        } => Node::AlternationPatternNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::AndNode {
            parser,
            pointer,
            marker,
        } => Node::AndNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ArgumentsNode {
            parser,
            pointer,
            marker,
        } => Node::ArgumentsNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ArrayNode {
            parser,
            pointer,
            marker,
        } => Node::ArrayNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ArrayPatternNode {
            parser,
            pointer,
            marker,
        } => Node::ArrayPatternNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::AssocNode {
            parser,
            pointer,
            marker,
        } => Node::AssocNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::AssocSplatNode {
            parser,
            pointer,
            marker,
        } => Node::AssocSplatNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::BackReferenceReadNode {
            parser,
            pointer,
            marker,
        } => Node::BackReferenceReadNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::BeginNode {
            parser,
            pointer,
            marker,
        } => Node::BeginNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::BlockArgumentNode {
            parser,
            pointer,
            marker,
        } => Node::BlockArgumentNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::BlockLocalVariableNode {
            parser,
            pointer,
            marker,
        } => Node::BlockLocalVariableNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::BlockNode {
            parser,
            pointer,
            marker,
        } => Node::BlockNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::BlockParameterNode {
            parser,
            pointer,
            marker,
        } => Node::BlockParameterNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::BlockParametersNode {
            parser,
            pointer,
            marker,
        } => Node::BlockParametersNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::BreakNode {
            parser,
            pointer,
            marker,
        } => Node::BreakNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::CallAndWriteNode {
            parser,
            pointer,
            marker,
        } => Node::CallAndWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::CallNode {
            parser,
            pointer,
            marker,
        } => Node::CallNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::CallOperatorWriteNode {
            parser,
            pointer,
            marker,
        } => Node::CallOperatorWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::CallOrWriteNode {
            parser,
            pointer,
            marker,
        } => Node::CallOrWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::CallTargetNode {
            parser,
            pointer,
            marker,
        } => Node::CallTargetNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::CapturePatternNode {
            parser,
            pointer,
            marker,
        } => Node::CapturePatternNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::CaseMatchNode {
            parser,
            pointer,
            marker,
        } => Node::CaseMatchNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::CaseNode {
            parser,
            pointer,
            marker,
        } => Node::CaseNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ClassNode {
            parser,
            pointer,
            marker,
        } => Node::ClassNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ClassVariableAndWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ClassVariableAndWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ClassVariableOperatorWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ClassVariableOperatorWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ClassVariableOrWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ClassVariableOrWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ClassVariableReadNode {
            parser,
            pointer,
            marker,
        } => Node::ClassVariableReadNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ClassVariableTargetNode {
            parser,
            pointer,
            marker,
        } => Node::ClassVariableTargetNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ClassVariableWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ClassVariableWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantAndWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantAndWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantOperatorWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantOperatorWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantOrWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantOrWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantPathAndWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantPathAndWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantPathNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantPathNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantPathOperatorWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantPathOperatorWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantPathOrWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantPathOrWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantPathTargetNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantPathTargetNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantPathWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantPathWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantReadNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantReadNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantTargetNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantTargetNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ConstantWriteNode {
            parser,
            pointer,
            marker,
        } => Node::ConstantWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::DefNode {
            parser,
            pointer,
            marker,
        } => Node::DefNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::DefinedNode {
            parser,
            pointer,
            marker,
        } => Node::DefinedNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ElseNode {
            parser,
            pointer,
            marker,
        } => Node::ElseNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::EmbeddedStatementsNode {
            parser,
            pointer,
            marker,
        } => Node::EmbeddedStatementsNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::EmbeddedVariableNode {
            parser,
            pointer,
            marker,
        } => Node::EmbeddedVariableNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::EnsureNode {
            parser,
            pointer,
            marker,
        } => Node::EnsureNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::FalseNode {
            parser,
            pointer,
            marker,
        } => Node::FalseNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::FindPatternNode {
            parser,
            pointer,
            marker,
        } => Node::FindPatternNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::FlipFlopNode {
            parser,
            pointer,
            marker,
        } => Node::FlipFlopNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::FloatNode {
            parser,
            pointer,
            marker,
        } => Node::FloatNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ForNode {
            parser,
            pointer,
            marker,
        } => Node::ForNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ForwardingArgumentsNode {
            parser,
            pointer,
            marker,
        } => Node::ForwardingArgumentsNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ForwardingParameterNode {
            parser,
            pointer,
            marker,
        } => Node::ForwardingParameterNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ForwardingSuperNode {
            parser,
            pointer,
            marker,
        } => Node::ForwardingSuperNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::GlobalVariableAndWriteNode {
            parser,
            pointer,
            marker,
        } => Node::GlobalVariableAndWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::GlobalVariableOperatorWriteNode {
            parser,
            pointer,
            marker,
        } => Node::GlobalVariableOperatorWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::GlobalVariableOrWriteNode {
            parser,
            pointer,
            marker,
        } => Node::GlobalVariableOrWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::GlobalVariableReadNode {
            parser,
            pointer,
            marker,
        } => Node::GlobalVariableReadNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::GlobalVariableTargetNode {
            parser,
            pointer,
            marker,
        } => Node::GlobalVariableTargetNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::GlobalVariableWriteNode {
            parser,
            pointer,
            marker,
        } => Node::GlobalVariableWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::HashNode {
            parser,
            pointer,
            marker,
        } => Node::HashNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::HashPatternNode {
            parser,
            pointer,
            marker,
        } => Node::HashPatternNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::IfNode {
            parser,
            pointer,
            marker,
        } => Node::IfNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ImaginaryNode {
            parser,
            pointer,
            marker,
        } => Node::ImaginaryNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ImplicitNode {
            parser,
            pointer,
            marker,
        } => Node::ImplicitNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ImplicitRestNode {
            parser,
            pointer,
            marker,
        } => Node::ImplicitRestNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InNode {
            parser,
            pointer,
            marker,
        } => Node::InNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::IndexAndWriteNode {
            parser,
            pointer,
            marker,
        } => Node::IndexAndWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::IndexOperatorWriteNode {
            parser,
            pointer,
            marker,
        } => Node::IndexOperatorWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::IndexOrWriteNode {
            parser,
            pointer,
            marker,
        } => Node::IndexOrWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::IndexTargetNode {
            parser,
            pointer,
            marker,
        } => Node::IndexTargetNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InstanceVariableAndWriteNode {
            parser,
            pointer,
            marker,
        } => Node::InstanceVariableAndWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InstanceVariableOperatorWriteNode {
            parser,
            pointer,
            marker,
        } => Node::InstanceVariableOperatorWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InstanceVariableOrWriteNode {
            parser,
            pointer,
            marker,
        } => Node::InstanceVariableOrWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InstanceVariableReadNode {
            parser,
            pointer,
            marker,
        } => Node::InstanceVariableReadNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InstanceVariableTargetNode {
            parser,
            pointer,
            marker,
        } => Node::InstanceVariableTargetNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InstanceVariableWriteNode {
            parser,
            pointer,
            marker,
        } => Node::InstanceVariableWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::IntegerNode {
            parser,
            pointer,
            marker,
        } => Node::IntegerNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InterpolatedMatchLastLineNode {
            parser,
            pointer,
            marker,
        } => Node::InterpolatedMatchLastLineNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InterpolatedRegularExpressionNode {
            parser,
            pointer,
            marker,
        } => Node::InterpolatedRegularExpressionNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InterpolatedStringNode {
            parser,
            pointer,
            marker,
        } => Node::InterpolatedStringNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InterpolatedSymbolNode {
            parser,
            pointer,
            marker,
        } => Node::InterpolatedSymbolNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::InterpolatedXStringNode {
            parser,
            pointer,
            marker,
        } => Node::InterpolatedXStringNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ItLocalVariableReadNode {
            parser,
            pointer,
            marker,
        } => Node::ItLocalVariableReadNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ItParametersNode {
            parser,
            pointer,
            marker,
        } => Node::ItParametersNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::KeywordHashNode {
            parser,
            pointer,
            marker,
        } => Node::KeywordHashNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::KeywordRestParameterNode {
            parser,
            pointer,
            marker,
        } => Node::KeywordRestParameterNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::LambdaNode {
            parser,
            pointer,
            marker,
        } => Node::LambdaNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::LocalVariableAndWriteNode {
            parser,
            pointer,
            marker,
        } => Node::LocalVariableAndWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::LocalVariableOperatorWriteNode {
            parser,
            pointer,
            marker,
        } => Node::LocalVariableOperatorWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::LocalVariableOrWriteNode {
            parser,
            pointer,
            marker,
        } => Node::LocalVariableOrWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::LocalVariableReadNode {
            parser,
            pointer,
            marker,
        } => Node::LocalVariableReadNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::LocalVariableTargetNode {
            parser,
            pointer,
            marker,
        } => Node::LocalVariableTargetNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::LocalVariableWriteNode {
            parser,
            pointer,
            marker,
        } => Node::LocalVariableWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::MatchLastLineNode {
            parser,
            pointer,
            marker,
        } => Node::MatchLastLineNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::MatchPredicateNode {
            parser,
            pointer,
            marker,
        } => Node::MatchPredicateNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::MatchRequiredNode {
            parser,
            pointer,
            marker,
        } => Node::MatchRequiredNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::MatchWriteNode {
            parser,
            pointer,
            marker,
        } => Node::MatchWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::MissingNode {
            parser,
            pointer,
            marker,
        } => Node::MissingNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ModuleNode {
            parser,
            pointer,
            marker,
        } => Node::ModuleNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::MultiTargetNode {
            parser,
            pointer,
            marker,
        } => Node::MultiTargetNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::MultiWriteNode {
            parser,
            pointer,
            marker,
        } => Node::MultiWriteNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::NextNode {
            parser,
            pointer,
            marker,
        } => Node::NextNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::NilNode {
            parser,
            pointer,
            marker,
        } => Node::NilNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::NoKeywordsParameterNode {
            parser,
            pointer,
            marker,
        } => Node::NoKeywordsParameterNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::NumberedParametersNode {
            parser,
            pointer,
            marker,
        } => Node::NumberedParametersNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::NumberedReferenceReadNode {
            parser,
            pointer,
            marker,
        } => Node::NumberedReferenceReadNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::OptionalKeywordParameterNode {
            parser,
            pointer,
            marker,
        } => Node::OptionalKeywordParameterNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::OptionalParameterNode {
            parser,
            pointer,
            marker,
        } => Node::OptionalParameterNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::OrNode {
            parser,
            pointer,
            marker,
        } => Node::OrNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ParametersNode {
            parser,
            pointer,
            marker,
        } => Node::ParametersNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ParenthesesNode {
            parser,
            pointer,
            marker,
        } => Node::ParenthesesNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::PinnedExpressionNode {
            parser,
            pointer,
            marker,
        } => Node::PinnedExpressionNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::PinnedVariableNode {
            parser,
            pointer,
            marker,
        } => Node::PinnedVariableNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::PostExecutionNode {
            parser,
            pointer,
            marker,
        } => Node::PostExecutionNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::PreExecutionNode {
            parser,
            pointer,
            marker,
        } => Node::PreExecutionNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ProgramNode {
            parser,
            pointer,
            marker,
        } => Node::ProgramNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::RangeNode {
            parser,
            pointer,
            marker,
        } => Node::RangeNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::RationalNode {
            parser,
            pointer,
            marker,
        } => Node::RationalNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::RedoNode {
            parser,
            pointer,
            marker,
        } => Node::RedoNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::RegularExpressionNode {
            parser,
            pointer,
            marker,
        } => Node::RegularExpressionNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::RequiredKeywordParameterNode {
            parser,
            pointer,
            marker,
        } => Node::RequiredKeywordParameterNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::RequiredParameterNode {
            parser,
            pointer,
            marker,
        } => Node::RequiredParameterNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::RescueModifierNode {
            parser,
            pointer,
            marker,
        } => Node::RescueModifierNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::RescueNode {
            parser,
            pointer,
            marker,
        } => Node::RescueNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::RestParameterNode {
            parser,
            pointer,
            marker,
        } => Node::RestParameterNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::RetryNode {
            parser,
            pointer,
            marker,
        } => Node::RetryNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ReturnNode {
            parser,
            pointer,
            marker,
        } => Node::ReturnNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::SelfNode {
            parser,
            pointer,
            marker,
        } => Node::SelfNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::ShareableConstantNode {
            parser,
            pointer,
            marker,
        } => Node::ShareableConstantNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::SingletonClassNode {
            parser,
            pointer,
            marker,
        } => Node::SingletonClassNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::SourceEncodingNode {
            parser,
            pointer,
            marker,
        } => Node::SourceEncodingNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::SourceFileNode {
            parser,
            pointer,
            marker,
        } => Node::SourceFileNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::SourceLineNode {
            parser,
            pointer,
            marker,
        } => Node::SourceLineNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::SplatNode {
            parser,
            pointer,
            marker,
        } => Node::SplatNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::StatementsNode {
            parser,
            pointer,
            marker,
        } => Node::StatementsNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::StringNode {
            parser,
            pointer,
            marker,
        } => Node::StringNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::SuperNode {
            parser,
            pointer,
            marker,
        } => Node::SuperNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::SymbolNode {
            parser,
            pointer,
            marker,
        } => Node::SymbolNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::TrueNode {
            parser,
            pointer,
            marker,
        } => Node::TrueNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::UndefNode {
            parser,
            pointer,
            marker,
        } => Node::UndefNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::UnlessNode {
            parser,
            pointer,
            marker,
        } => Node::UnlessNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::UntilNode {
            parser,
            pointer,
            marker,
        } => Node::UntilNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::WhenNode {
            parser,
            pointer,
            marker,
        } => Node::WhenNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::WhileNode {
            parser,
            pointer,
            marker,
        } => Node::WhileNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::XStringNode {
            parser,
            pointer,
            marker,
        } => Node::XStringNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
        Node::YieldNode {
            parser,
            pointer,
            marker,
        } => Node::YieldNode {
            parser: *parser,
            pointer: *pointer,
            marker: *marker,
        },
    }
}

/// Atoms: names and values that are not child nodes. `CallNode::name` lives
/// here -- comparing variant and children alone would match `foo(a)`
/// against `bar(a)`.
#[allow(clippy::too_many_lines, unused_variables)]
pub(crate) fn atoms<'pr>(node: &Node<'pr>) -> Vec<Atom> {
    let mut out = Vec::new();
    match node {
        Node::BackReferenceReadNode { .. } => {
            let Some(n) = node.as_back_reference_read_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::BlockLocalVariableNode { .. } => {
            let Some(n) = node.as_block_local_variable_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::BlockParameterNode { .. } => {
            let Some(n) = node.as_block_parameter_node() else {
                return out;
            };
            if let Some(c) = n.name() {
                out.push(Atom::name(c.as_slice()));
            }
        }
        Node::CallAndWriteNode { .. } => {
            let Some(n) = node.as_call_and_write_node() else {
                return out;
            };
            out.push(Atom::name(n.read_name().as_slice()));
            out.push(Atom::name(n.write_name().as_slice()));
        }
        Node::CallNode { .. } => {
            let Some(n) = node.as_call_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::CallOperatorWriteNode { .. } => {
            let Some(n) = node.as_call_operator_write_node() else {
                return out;
            };
            out.push(Atom::name(n.read_name().as_slice()));
            out.push(Atom::name(n.write_name().as_slice()));
            out.push(Atom::name(n.binary_operator().as_slice()));
        }
        Node::CallOrWriteNode { .. } => {
            let Some(n) = node.as_call_or_write_node() else {
                return out;
            };
            out.push(Atom::name(n.read_name().as_slice()));
            out.push(Atom::name(n.write_name().as_slice()));
        }
        Node::CallTargetNode { .. } => {
            let Some(n) = node.as_call_target_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ClassNode { .. } => {
            let Some(n) = node.as_class_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ClassVariableAndWriteNode { .. } => {
            let Some(n) = node.as_class_variable_and_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ClassVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_class_variable_operator_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
            out.push(Atom::name(n.binary_operator().as_slice()));
        }
        Node::ClassVariableOrWriteNode { .. } => {
            let Some(n) = node.as_class_variable_or_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ClassVariableReadNode { .. } => {
            let Some(n) = node.as_class_variable_read_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ClassVariableTargetNode { .. } => {
            let Some(n) = node.as_class_variable_target_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ClassVariableWriteNode { .. } => {
            let Some(n) = node.as_class_variable_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ConstantAndWriteNode { .. } => {
            let Some(n) = node.as_constant_and_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ConstantOperatorWriteNode { .. } => {
            let Some(n) = node.as_constant_operator_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
            out.push(Atom::name(n.binary_operator().as_slice()));
        }
        Node::ConstantOrWriteNode { .. } => {
            let Some(n) = node.as_constant_or_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ConstantPathNode { .. } => {
            let Some(n) = node.as_constant_path_node() else {
                return out;
            };
            if let Some(c) = n.name() {
                out.push(Atom::name(c.as_slice()));
            }
        }
        Node::ConstantPathOperatorWriteNode { .. } => {
            let Some(n) = node.as_constant_path_operator_write_node() else {
                return out;
            };
            out.push(Atom::name(n.binary_operator().as_slice()));
        }
        Node::ConstantPathTargetNode { .. } => {
            let Some(n) = node.as_constant_path_target_node() else {
                return out;
            };
            if let Some(c) = n.name() {
                out.push(Atom::name(c.as_slice()));
            }
        }
        Node::ConstantReadNode { .. } => {
            let Some(n) = node.as_constant_read_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ConstantTargetNode { .. } => {
            let Some(n) = node.as_constant_target_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::ConstantWriteNode { .. } => {
            let Some(n) = node.as_constant_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::DefNode { .. } => {
            let Some(n) = node.as_def_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::FloatNode { .. } => {
            let Some(n) = node.as_float_node() else {
                return out;
            };
            out.push(Atom::debug(&n.value()));
        }
        Node::GlobalVariableAndWriteNode { .. } => {
            let Some(n) = node.as_global_variable_and_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::GlobalVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_global_variable_operator_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
            out.push(Atom::name(n.binary_operator().as_slice()));
        }
        Node::GlobalVariableOrWriteNode { .. } => {
            let Some(n) = node.as_global_variable_or_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::GlobalVariableReadNode { .. } => {
            let Some(n) = node.as_global_variable_read_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::GlobalVariableTargetNode { .. } => {
            let Some(n) = node.as_global_variable_target_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::GlobalVariableWriteNode { .. } => {
            let Some(n) = node.as_global_variable_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::IndexOperatorWriteNode { .. } => {
            let Some(n) = node.as_index_operator_write_node() else {
                return out;
            };
            out.push(Atom::name(n.binary_operator().as_slice()));
        }
        Node::InstanceVariableAndWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_and_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::InstanceVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_operator_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
            out.push(Atom::name(n.binary_operator().as_slice()));
        }
        Node::InstanceVariableOrWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_or_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::InstanceVariableReadNode { .. } => {
            let Some(n) = node.as_instance_variable_read_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::InstanceVariableTargetNode { .. } => {
            let Some(n) = node.as_instance_variable_target_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::InstanceVariableWriteNode { .. } => {
            let Some(n) = node.as_instance_variable_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::IntegerNode { .. } => {
            let Some(n) = node.as_integer_node() else {
                return out;
            };
            out.push(Atom::integer(&n.value()));
        }
        Node::KeywordRestParameterNode { .. } => {
            let Some(n) = node.as_keyword_rest_parameter_node() else {
                return out;
            };
            if let Some(c) = n.name() {
                out.push(Atom::name(c.as_slice()));
            }
        }
        Node::LocalVariableAndWriteNode { .. } => {
            let Some(n) = node.as_local_variable_and_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
            out.push(Atom::debug(&n.depth()));
        }
        Node::LocalVariableOperatorWriteNode { .. } => {
            let Some(n) = node.as_local_variable_operator_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
            out.push(Atom::name(n.binary_operator().as_slice()));
            out.push(Atom::debug(&n.depth()));
        }
        Node::LocalVariableOrWriteNode { .. } => {
            let Some(n) = node.as_local_variable_or_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
            out.push(Atom::debug(&n.depth()));
        }
        Node::LocalVariableReadNode { .. } => {
            let Some(n) = node.as_local_variable_read_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
            out.push(Atom::debug(&n.depth()));
        }
        Node::LocalVariableTargetNode { .. } => {
            let Some(n) = node.as_local_variable_target_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
            out.push(Atom::debug(&n.depth()));
        }
        Node::LocalVariableWriteNode { .. } => {
            let Some(n) = node.as_local_variable_write_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
            out.push(Atom::debug(&n.depth()));
        }
        Node::MatchLastLineNode { .. } => {
            let Some(n) = node.as_match_last_line_node() else {
                return out;
            };
            out.push(Atom::value(n.unescaped()));
        }
        Node::ModuleNode { .. } => {
            let Some(n) = node.as_module_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::NumberedParametersNode { .. } => {
            let Some(n) = node.as_numbered_parameters_node() else {
                return out;
            };
            out.push(Atom::debug(&n.maximum()));
        }
        Node::NumberedReferenceReadNode { .. } => {
            let Some(n) = node.as_numbered_reference_read_node() else {
                return out;
            };
            out.push(Atom::debug(&n.number()));
        }
        Node::OptionalKeywordParameterNode { .. } => {
            let Some(n) = node.as_optional_keyword_parameter_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::OptionalParameterNode { .. } => {
            let Some(n) = node.as_optional_parameter_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::RationalNode { .. } => {
            let Some(n) = node.as_rational_node() else {
                return out;
            };
            out.push(Atom::integer(&n.numerator()));
            out.push(Atom::integer(&n.denominator()));
        }
        Node::RegularExpressionNode { .. } => {
            let Some(n) = node.as_regular_expression_node() else {
                return out;
            };
            out.push(Atom::value(n.unescaped()));
        }
        Node::RequiredKeywordParameterNode { .. } => {
            let Some(n) = node.as_required_keyword_parameter_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::RequiredParameterNode { .. } => {
            let Some(n) = node.as_required_parameter_node() else {
                return out;
            };
            out.push(Atom::name(n.name().as_slice()));
        }
        Node::RestParameterNode { .. } => {
            let Some(n) = node.as_rest_parameter_node() else {
                return out;
            };
            if let Some(c) = n.name() {
                out.push(Atom::name(c.as_slice()));
            }
        }
        Node::SourceFileNode { .. } => {
            let Some(n) = node.as_source_file_node() else {
                return out;
            };
            out.push(Atom::value(n.filepath()));
        }
        Node::StringNode { .. } => {
            let Some(n) = node.as_string_node() else {
                return out;
            };
            out.push(Atom::value(n.unescaped()));
        }
        Node::SymbolNode { .. } => {
            let Some(n) = node.as_symbol_node() else {
                return out;
            };
            out.push(Atom::value(n.unescaped()));
        }
        Node::XStringNode { .. } => {
            let Some(n) = node.as_x_string_node() else {
                return out;
            };
            out.push(Atom::value(n.unescaped()));
        }
        _ => {}
    }
    out
}
