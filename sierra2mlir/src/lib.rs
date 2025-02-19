//! A compiler to convert Cairo's intermediate representation "Sierra" code to MLIR.

//#![deny(warnings)]
#![warn(clippy::nursery)]
//#![deny(clippy::pedantic)]
#![warn(clippy::all)]
#![warn(unused)]
#![allow(clippy::too_many_arguments)]

use melior_next::{pass, utility::register_all_passes, ExecutionEngine};
use tracing::debug;

use self::utility::run_llvm_config;
use crate::compiler::Compiler;
use cairo_lang_sierra::program::Program;

pub mod compiler;
mod libfuncs;
mod sierra_type;
mod statements;
pub mod types;
mod userfuncs;
mod utility;

pub fn compile(
    program: &Program,
    optimized: bool,
    debug_info: bool,
    // TODO: Make this an enum with either: stdout, stderr, a path to a file, or a raw fd (pipes?).
    main_print: bool,
    print_fd: i32,
    available_gas: Option<usize>,
) -> Result<String, color_eyre::Report> {
    let mut compiler = Compiler::new(program, main_print, print_fd, available_gas)?;
    compiler.compile()?;

    debug!("mlir before pass:\n{}", compiler.module.as_operation());
    let pass_manager = pass::Manager::new(&compiler.context);
    register_all_passes();

    pass_manager.add_pass(pass::conversion::convert_func_to_llvm());
    pass_manager.add_pass(pass::conversion::convert_scf_to_cf());
    pass_manager.add_pass(pass::conversion::convert_cf_to_llvm());
    pass_manager.add_pass(pass::conversion::convert_arithmetic_to_llvm());
    pass_manager.add_pass(pass::conversion::convert_index_to_llvm());
    pass_manager.add_pass(pass::conversion::convert_math_to_llvm());
    pass_manager.add_pass(pass::conversion::convert_memref_to_llvmconversion_pass());
    pass_manager.add_pass(pass::conversion::convert_reconcile_unrealized_casts());

    if optimized {
        pass_manager.add_pass(pass::transform::canonicalizer());
        pass_manager.add_pass(pass::transform::inliner());
        pass_manager.add_pass(pass::transform::symbol_dce());
        pass_manager.add_pass(pass::transform::cse());
        pass_manager.add_pass(pass::transform::sccp());
    }

    // pass_manager.add_pass(pass::transform::print_operation_stats());
    pass_manager.enable_verifier(true);
    pass_manager.run(&mut compiler.module)?;

    let op = compiler.module.as_operation();
    if op.verify() {
        if debug_info {
            Ok(op.debug_print())
        } else {
            Ok(op.to_string())
        }
    } else {
        Err(color_eyre::eyre::eyre!("error verifiying"))
    }
}

pub fn execute(
    program: &Program,
    main_print: bool,
    print_fd: i32,
    available_gas: Option<usize>,
) -> Result<ExecutionEngine, color_eyre::Report> {
    let mut compiler = Compiler::new(program, main_print, print_fd, available_gas)?;
    compiler.compile()?;

    let pass_manager = pass::Manager::new(&compiler.context);
    register_all_passes();

    pass_manager.add_pass(pass::conversion::convert_func_to_llvm());
    pass_manager.add_pass(pass::conversion::convert_scf_to_cf());
    pass_manager.add_pass(pass::conversion::convert_cf_to_llvm());
    pass_manager.add_pass(pass::conversion::convert_arithmetic_to_llvm());
    pass_manager.add_pass(pass::conversion::convert_index_to_llvm());
    pass_manager.add_pass(pass::conversion::convert_math_to_llvm());
    pass_manager.add_pass(pass::conversion::convert_memref_to_llvmconversion_pass());
    pass_manager.add_pass(pass::conversion::convert_reconcile_unrealized_casts());

    pass_manager.add_pass(pass::transform::canonicalizer());
    pass_manager.add_pass(pass::transform::inliner());
    pass_manager.add_pass(pass::transform::symbol_dce());
    pass_manager.add_pass(pass::transform::cse());
    pass_manager.add_pass(pass::transform::sccp());

    pass_manager.enable_verifier(true);
    pass_manager.run(&mut compiler.module)?;

    let engine = ExecutionEngine::new(
        &compiler.module,
        2,
        &[
            &format!(
                "{}/libmlir_c_runner_utils.{}",
                run_llvm_config(&["--libdir"]).trim(),
                env!("SHARED_LIB_EXT"),
            ),
            env!("S2M_UTILS_PATH"),
        ],
        false,
    );

    Ok(engine)
}
