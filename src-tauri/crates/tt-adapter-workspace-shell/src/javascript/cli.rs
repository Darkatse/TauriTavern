pub(super) const HELP: &str = r#"JavaScript (ES modules):
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
Indexes are 0-based over the current character chat's visible history; the run's frozen upper bound is totalMessages.
getMessage returns one message; getMessages reads many in a single file scan, up to 500 per call.
A read has no line cap: the per-message byte budget is the only server-side bound.
A window the byte budget ended before the requested last line, or a single line it clipped, sets preview: true; continue with startLine.
Failures do not throw: check result.ok and result.reason, one of chat.not_found, chat.message_not_found,
chat.invalid_message_range, chat.message_too_large, chat.unsupported. Malformed arguments and failed reads do throw.
A read without a range of a message over 1048576 bytes returns chat.message_too_large, reporting totalBytes and maxBytes;
reread it with startLine/lineCount, which is not subject to that cap, and save long text to a workspace file.
The Profile must grant chat.read_messages; otherwise valid calls return chat.unsupported.
Group chats and non-character runs report chat.unsupported the same way.

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
