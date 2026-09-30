use tt_ports::workspace_shell::{SCRIPT_MAX_CALL_BYTES, SCRIPT_MAX_MESSAGE_BYTES};

/// `js --help` text, with one placeholder per script-visible chat budget.
///
/// [`help`] fills them from the port constants, so the numbers the model reads cannot
/// drift from the limits the shell enforces.
const HELP_TEMPLATE: &str = r#"JavaScript (ES modules):
  js -e 'console.log(1 + 2)'
  js /scratch/task.js arg1 --option 'arg with spaces'
  js -- /scratch/task.js arg1
  js - arg1 < /scratch/task.js
  js -e 'console.log(JSON.stringify(process.argv.slice(1)))' -- --help

Arguments:
File and stdin entries end interpreter options; all following arguments belong to the script.
With -e/--eval, interpreter options end at -- or the first non-option argument.
process.argv contains [command, entry, ...args]; entry is the resolved workspace path or '-'.
Eval has no entry: process.argv contains [command, ...args]. All arguments remain strings.
Use js --help for interpreter help, or js FILE.js --help for a script's own help.
Modules execute their top-level code; call your functions explicitly and await asynchronous work.

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

Chat messages:
  import { chat } from '@tauritavern/runtime';
  const message = chat.getMessage(3);                 // {ok, index, role, name, sendDate, text, ref, startLine, endLine, totalLines, totalBytes, preview, totalMessages}
  const part = chat.getMessage(3, { startLine: 10, lineCount: 20 });
  const batch = chat.getMessages([3, 4, 5]);          // {ok, messages: [...], totalMessages}
Indexes are 0-based over the current character chat's visible history; totalMessages is the run's frozen upper bound.
getMessage returns one message; getMessages reads many in a single file scan. A read has no line cap.
Options: setting lineCount requires startLine; startLine alone reads to the end; omitting both reads the whole message.
The chat_read_messages tool (chat.read_messages in a Profile) follows the same rule; an empty index list is malformed.
One call is bounded by {script_max_message_bytes} bytes per message and {script_max_call_bytes} bytes per call, charged as each returned message's text
plus a fixed entry overhead - that overhead is what bounds the batch size, so asking for more indexes than it covers throws.
Chat bytes also count against the shell's aggregate input budget and deadline, the same one file reads use.
preview: true means the budget ended the window before the requested last line, or clipped a single line; continue with startLine.
The tool's (preview) also covers a window that stops before the message's end, so the two verdicts differ.
Failures do not throw: check result.ok and result.reason, one of chat.not_found, chat.message_not_found,
chat.invalid_message_range, chat.message_too_large, chat.call_too_large, chat.unsupported. Malformed arguments and failed reads do throw.
A message over {script_max_message_bytes} bytes read without a range returns chat.message_too_large (totalBytes, maxBytes); a call over
{script_max_call_bytes} bytes returns chat.call_too_large (usedBytes, maxBytes). Reread with startLine/lineCount, which has no per-message cap.
chat.unsupported means the capability is absent here: the Profile did not grant chat.read_messages, or the run is a group chat or not a character chat.

Output:
console.log/info/debug write stdout; console.warn/error write stderr.
Print structured results with console.log(JSON.stringify(result)); use workspace files for large inputs and outputs.
For logging to stderr, import { log } from '@tauritavern/runtime'.
process.exitCode defaults to 0; assign a numeric integer from 0 to 255 to set the exit status.
Uncaught errors fail the command even if exitCode is 0. process.exit() is unavailable.

Built-in libraries: @tauritavern/kit/{dayjs,es-toolkit,fast-xml-parser,marked,papaparse,slugify}.
Aliases: node uses js syntax; deno run FILE [args...] and deno eval SOURCE [--] [args...] use the same environment.
Node/Deno libraries, npm, TypeScript, network and child processes are unavailable.
"#;

/// Fill [`HELP_TEMPLATE`] with the budgets the shell actually enforces.
///
/// The budgets are substituted by string key because the help text is one raw block
/// that carries literal braces (`{ log }`, `{dayjs,...}`), which `format!` would read
/// as placeholders. The cost is that a key can be misspelled without anything failing:
/// `replace` leaves the placeholder in place and the text still looks plausible, which
/// no test asserts. Keep the keys here and in the template identical.
pub(super) fn help() -> String {
    HELP_TEMPLATE
        .replace(
            "{script_max_message_bytes}",
            &SCRIPT_MAX_MESSAGE_BYTES.to_string(),
        )
        .replace(
            "{script_max_call_bytes}",
            &SCRIPT_MAX_CALL_BYTES.to_string(),
        )
}

pub(super) enum Source {
    File(String),
    Eval(String),
    Stdin(String),
}

pub(super) struct Script {
    pub command: String,
    pub source: Source,
    pub args: Vec<String>,
}

pub(super) enum Command {
    Help,
    Run(Script),
}

pub(super) fn parse(name: &str, args: &[String], stdin: Option<&[u8]>) -> Result<Command, String> {
    let mut args = args.iter();
    let mut source = None;
    if name == "deno" {
        match args.next().map(String::as_str) {
            Some("run") => {}
            Some("eval") => source = Some(Source::Eval(value(&mut args, "deno eval")?)),
            Some("--help" | "-h") => return Ok(Command::Help),
            _ => {
                return Err("Use deno run FILE or deno eval SOURCE. See js --help.".into());
            }
        }
    }

    let mut script_args = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(Command::Help),
            "-e" | "--eval" if source.is_none() => {
                source = Some(Source::Eval(value(&mut args, arg)?));
            }
            "--" => {
                if source.is_none() {
                    source = Some(entry(&value(&mut args, "--")?, stdin)?);
                }
                break;
            }
            arg if arg == "-" || !arg.starts_with('-') => {
                if source.is_none() {
                    source = Some(entry(arg, stdin)?);
                } else {
                    script_args.push(arg.to_owned());
                }
                // File/stdin entries end options; eval ends them at its first operand.
                break;
            }
            _ => {
                return Err(format!(
                    "Unsupported or repeated argument `{arg}`. See js --help."
                ));
            }
        }
    }
    let source =
        source.ok_or("Provide a script file, -e SOURCE, or - for stdin. See js --help.")?;
    script_args.extend(args.cloned());
    Ok(Command::Run(Script {
        command: name.to_owned(),
        source,
        args: script_args,
    }))
}

fn entry(path: &str, stdin: Option<&[u8]>) -> Result<Source, String> {
    if path != "-" {
        return Ok(Source::File(path.to_owned()));
    }
    let bytes = stdin.ok_or("js - requires module source on stdin.")?;
    if bytes.len() > crate::engine::MAX_COMMAND_BYTES {
        return Err(
            "JavaScript stdin exceeds the command size limit; use a workspace script file.".into(),
        );
    }
    String::from_utf8(bytes.to_vec())
        .map(Source::Stdin)
        .map_err(|_| "JavaScript source must be UTF-8.".into())
}

fn value<'a>(args: &mut impl Iterator<Item = &'a String>, option: &str) -> Result<String, String> {
    args.next()
        .cloned()
        .ok_or_else(|| format!("{option} requires a value. See js --help."))
}
