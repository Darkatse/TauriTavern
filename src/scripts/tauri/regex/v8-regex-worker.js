// @ts-check

import { planReplace } from './gated-replace.js';

/**
 * Applies scripts whose replacement needs no main-thread macro substitution.
 * @param {{ text: string, scripts: any[] }[]} tasks
 * @param {(script: any) => void} onScriptStart Announces a script whose regex is about to run
 * @returns {{ text: string }[]}
 */
export function applyV8RegexTasks(tasks, onScriptStart) {
    return tasks.map(task => {
        let text = task.text;

        for (const script of task.scripts) {
            const replace = planReplace(text, new RegExp(script.pattern, script.flags));
            if (replace === null) {
                continue;
            }
            onScriptStart(script);
            text = replace((...args) => script.replacement.replaceAll(
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
