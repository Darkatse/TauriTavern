// @ts-check

/**
 * Applies the portable regex subset shared with the native backend.
 * @param {{ text: string, scripts: any[] }[]} tasks
 * @param {(script: any) => void} onScriptStart
 * @returns {{ text: string }[]}
 */
export function applyV8RegexTasks(tasks, onScriptStart) {
    return tasks.map(task => {
        let text = task.text;

        for (const script of task.scripts) {
            if (script.requiredLiteral && !text.includes(script.requiredLiteral)) {
                continue;
            }

            onScriptStart(script);
            const regex = new RegExp(script.pattern, script.flags);
            text = text.replace(regex, (...args) => script.replacement.replaceAll(
                /\$(\d+)|\$<([^>]+)>/g,
                (_, index, name) => {
                    const groups = args.at(-1);
                    const capture = index
                        ? args[Number(index)]
                        : (groups && typeof groups === 'object' ? groups[name] : undefined);
                    return capture
                        ? script.trimStrings.reduce((value, trim) => value.replaceAll(trim, ''), capture)
                        : '';
                },
            ));
        }

        return { text };
    });
}

if (typeof self !== 'undefined') {
    self.onmessage = event => {
        try {
            const tasks = applyV8RegexTasks(event.data.tasks, script => {
                self.postMessage({
                    type: 'script-start',
                    scriptKey: script.scriptKey,
                    scriptName: script.scriptName,
                    allowSlow: script.allowSlow,
                });
            });
            self.postMessage({ type: 'result', tasks });
        } catch (error) {
            self.postMessage({
                type: 'error',
                message: error instanceof Error ? error.message : String(error),
            });
        }
    };
}
