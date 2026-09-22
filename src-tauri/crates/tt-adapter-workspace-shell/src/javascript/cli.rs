use serde_json::Value;

pub(super) const HELP: &str = r#"JavaScript (ES modules):
  js -e 'console.log(1 + 2)'
  js /scratch/task.js arg1 arg2
  js - < /scratch/task.js
  js --call EXPORT [--args-json OBJECT] FILE.js

Arguments after the entry point are exposed as process.argv:
  js /scratch/task.js a b    -> process.argv = ['js', '/scratch/task.js', 'a', 'b']
  js -e '...' a b            -> process.argv = ['js', 'a', 'b']

Workspace files:
  import { workspace } from '@tauritavern/runtime';
  const text = workspace.readText('scratch/input.txt');
  workspace.writeText('output/result.txt', text.toUpperCase());

readText(path) reads UTF-8 text; writeText(path, text) creates or replaces a file.
exists(path) checks whether a path is accessible and exists.
listFiles() lists workspace roots. listFiles(directory) lists files recursively, with paths relative to that directory.
File API paths start at the workspace root. Script paths use the shell working directory; relative imports use the importing file's directory. Module files need a .js or .mjs extension.

Chat context:
  import { context, macros } from '@tauritavern/runtime';
context.worldInfo.entries, context.variables.local/global and context.macro contain chat values captured when the run started. Unavailable fields throw an error.
macros.render(text) expands chat macros.

Output:
console.log/info/debug write stdout; console.warn/error write stderr.
--call passes OBJECT (default {}) to the export and writes its JSON result to stdout; all logs go to stderr.
For logging to stderr in either mode, import { log } from '@tauritavern/runtime'.

Built-in libraries: @tauritavern/kit/{dayjs,es-toolkit,fast-xml-parser,marked,papaparse,slugify}.
Aliases: node uses js syntax; deno run FILE and deno eval SOURCE use the same environment.
Node/Deno libraries, npm, TypeScript, network and child processes are unavailable.
"#;

pub(super) enum Source {
    Code(String),
    File(String),
}

pub(super) struct Script {
    /// The invoked builtin name (`js`, `node`, or `deno`), used as `process.argv[0]`.
    pub name: String,
    pub source: Source,
    /// The entry point shown as `process.argv[1]`; `None` for `-e` (Node keeps
    /// eval arguments directly after the command name).
    pub entry: Option<String>,
    pub call: Option<String>,
    pub args: Value,
    /// Positional arguments after the entry point, exposed as `process.argv`.
    pub rest: Vec<String>,
}

pub(super) enum Command {
    Help,
    Run(Script),
}

pub(super) fn parse(name: &str, args: &[String], stdin: Option<&[u8]>) -> Result<Command, String> {
    if args == ["--help"] || args == ["-h"] {
        return Ok(Command::Help);
    }
    if name == "deno" {
        return match args.split_first() {
            Some((command, rest)) if command == "run" => parse("js", rest, stdin),
            Some((command, rest)) if command == "eval" && rest.len() == 1 => {
                parse("js", &["-e".into(), rest[0].clone()], stdin)
            }
            _ => Err("Use deno run FILE or deno eval SOURCE. See js --help.".into()),
        };
    }

    let mut source = None;
    let mut entry = None;
    let mut call = None;
    let mut json = None;
    let mut rest: Vec<String> = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-e" | "--eval" if source.is_none() => {
                source = Some(Source::Code(value(&mut args, arg)?));
            }
            "--call" if call.is_none() => call = Some(value(&mut args, arg)?),
            "--args-json" if json.is_none() => {
                let raw = value(&mut args, arg)?;
                let parsed: Value = serde_json::from_str(&raw)
                    .map_err(|error| format!("--args-json must be a JSON object: {error}"))?;
                if !parsed.is_object() {
                    return Err("--args-json must be a JSON object.".into());
                }
                json = Some(parsed);
            }
            "-" if source.is_none() => {
                let bytes = stdin.ok_or("js - requires module source on stdin.")?;
                if bytes.len() > crate::engine::MAX_COMMAND_BYTES {
                    return Err("JavaScript stdin exceeds the command size limit; use a workspace script file.".into());
                }
                source = Some(Source::Code(
                    String::from_utf8(bytes.to_vec())
                        .map_err(|_| "JavaScript source must be UTF-8.")?,
                ));
                entry = Some("-".into());
            }
            path if !path.starts_with('-') && source.is_none() => {
                source = Some(Source::File(path.to_owned()));
                entry = Some(path.to_owned());
            }
            arg if !arg.starts_with('-') => rest.push(arg.to_owned()),
            _ => {
                return Err(format!(
                    "Unsupported or repeated argument `{arg}`. See js --help."
                ));
            }
        }
    }
    let source =
        source.ok_or("Provide a script file, -e SOURCE, or - for stdin. See js --help.")?;
    if json.is_some() && call.is_none() {
        return Err("--args-json requires --call EXPORT.".into());
    }
    Ok(Command::Run(Script {
        name: name.to_owned(),
        source,
        entry,
        call,
        args: json.unwrap_or_else(|| serde_json::json!({})),
        rest,
    }))
}

fn value<'a>(args: &mut impl Iterator<Item = &'a String>, option: &str) -> Result<String, String> {
    args.next()
        .cloned()
        .ok_or_else(|| format!("{option} requires a value. See js --help."))
}
