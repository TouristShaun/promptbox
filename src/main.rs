use std::{ffi::OsString, path::PathBuf};

use args::{parse_main_args, parse_template_args, FoundCommand, GlobalRunArgs};
use config::Config;
use error::Error;
use error_stack::{Report, ResultExt};
use global_config::load_dotenv;
use liquid::partials::{InMemorySource, LazyCompiler};
use model::ModelOptions;
use template::ParsedTemplate;

use crate::model::send_model_request;

mod args;
mod config;
mod error;
mod global_config;
mod model;
mod ollama;
mod openai;
mod option;
mod template;
#[cfg(test)]
mod tests;

fn generate_template(
    base_dir: PathBuf,
    template: String,
    cmdline: Vec<OsString>,
) -> Result<(GlobalRunArgs, ModelOptions, String), Report<Error>> {
    let config = Config::from_directory(base_dir.clone())?;

    let ParsedTemplate {
        template,
        path: template_path,
        input,
        ..
    } = config.find_template(&template)?;

    let (args, template_context) = parse_template_args(cmdline, &base_dir, &input)?;

    let template = match (args.prepend.as_ref(), args.append.as_ref()) {
        (Some(pre), Some(post)) => format!("{pre}\n\n{template}\n\n{post}"),
        (Some(pre), None) => format!("{pre}\n\n{template}"),
        (None, Some(post)) => format!("{template}\n\n{post}"),
        (None, None) => template,
    };

    // TODO replace InMemorySource with a custom source that can look for partials in the various
    // config directories.
    let parser = liquid::ParserBuilder::<LazyCompiler<InMemorySource>>::default()
        .stdlib()
        .build()
        .expect("failed to build parser");

    let parsed = parser
        .parse(&template)
        .change_context(Error::ParseTemplate)
        .attach_printable_lazy(|| template_path.display().to_string())?;

    let prompt = parsed
        .render(&template_context)
        .change_context(Error::ParseTemplate)
        .attach_printable_lazy(|| template_path.display().to_string())?;

    let mut model_options = config.model;
    model_options.update_from_args(&args);

    Ok((args, model_options, prompt))
}

fn run_template(
    base_dir: PathBuf,
    template: String,
    args: Vec<OsString>,
) -> Result<(), Report<Error>> {
    let (args, model_options, prompt) = generate_template(base_dir, template, args)?;

    if args.print_prompt || args.verbose || args.dry_run {
        println!("{}\n\nResult:", prompt);
    }

    if args.dry_run {
        return Ok(());
    }

    let result = send_model_request(&model_options, &prompt).change_context(Error::RunPrompt)?;

    println!("{}", result);

    Ok(())
}

fn run(base_dir: PathBuf, cmdline: Vec<OsString>) -> Result<(), Report<Error>> {
    let args = parse_main_args(cmdline).change_context(Error::ArgParseFailure)?;

    match args {
        FoundCommand::Run { template, args } => run_template(base_dir, template, args)?,
        FoundCommand::Other(cli) => {
            todo!()
        }
    }

    Ok(())
}

fn main() -> Result<(), Report<Error>> {
    // Don't show file locations in normal mode
    #[cfg(not(debug_assertions))]
    error_stack::Report::install_debug_hook::<std::panic::Location>(|_, _| {});

    load_dotenv();
    run(
        std::env::current_dir().unwrap(),
        std::env::args().into_iter().map(OsString::from).collect(),
    )
}
