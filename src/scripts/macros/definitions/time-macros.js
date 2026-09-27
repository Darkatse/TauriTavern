import { moment } from '../../../lib.js';
import { MacroRegistry, MacroCategory, MacroValueType } from '../engine/MacroRegistry.js';

/**
 * Registers time/date related macros and utilities.
 */
export function registerTimeMacros() {
    // Time and date macros
    MacroRegistry.registerMacro('time', {
        category: MacroCategory.TIME,
        // Optional single list argument: UTC offset, e.g. {{time::UTC+2}}
        unnamedArgs: [
            {
                name: 'offset',
                optional: true,
                defaultValue: 'null',
                type: MacroValueType.STRING,
                sampleValue: 'UTC+2',
                description: 'UTC offset in the format UTC±(offset).',
            },
        ],
        description: 'Current local time, or UTC offset when called as {{time::UTC±(offset)}}',
        returns: 'A time string in the format HH:mm.',
        displayOverride: '{{time::[UTC±(offset)]}}',
        exampleUsage: ['{{time}}', '{{time::UTC+2}}', '{{time::UTC-7}}'],
        handler: ({ env, unnamedArgs: [offsetSpec] }) => {
            if (!offsetSpec) return moment(env.now).format('LT');

            const match = /^UTC([+-]\d+)$/.exec(offsetSpec);
            if (!match) return moment(env.now).format('LT');

            const offset = Number.parseInt(match[1], 10);
            if (Number.isNaN(offset)) return moment(env.now).format('LT');

            return moment(env.now).utc().utcOffset(offset).format('LT');
        },
    });

    MacroRegistry.registerMacro('date', {
        category: MacroCategory.TIME,
        description: 'Current local date as a string in the local short format.',
        returns: 'Current local date in local short format.',
        handler: ({ env }) => moment(env.now).format('LL'),
    });

    MacroRegistry.registerMacro('weekday', {
        category: MacroCategory.TIME,
        description: 'Current weekday name.',
        returns: 'Current weekday name.',
        handler: ({ env }) => moment(env.now).format('dddd'),
    });

    MacroRegistry.registerMacro('isotime', {
        category: MacroCategory.TIME,
        description: 'Current time in HH:mm format.',
        returns: 'Current time in HH:mm format.',
        handler: ({ env }) => moment(env.now).format('HH:mm'),
    });

    MacroRegistry.registerMacro('isodate', {
        category: MacroCategory.TIME,
        description: 'Current date in YYYY-MM-DD format.',
        returns: 'Current date in YYYY-MM-DD format.',
        handler: ({ env }) => moment(env.now).format('YYYY-MM-DD'),
    });

    MacroRegistry.registerMacro('datetimeformat', {
        category: MacroCategory.TIME,
        unnamedArgs: [
            {
                name: 'format',
                sampleValue: 'YYYY-MM-DD HH:mm:ss',
                description: 'Moment.js format string.',
                type: 'string',
            },
        ],
        description: 'Formats the current date/time using the given moment.js format string.',
        returns: 'Formatted date/time string.',
        exampleUsage: ['{{datetimeformat::YYYY-MM-DD HH:mm:ss}}', '{{datetimeformat::LLLL}}'],
        handler: ({ env, unnamedArgs: [format] }) => moment(env.now).format(format),
    });

    MacroRegistry.registerMacro('idleDuration', {
        aliases: [{ alias: 'idle_duration', visible: false }],
        category: MacroCategory.TIME,
        description: 'Human-readable duration since the last user message.',
        returns: 'Human-readable duration since the last user message.',
        handler: ({ env }) => env.chat.idleDuration,
    });

    // Time difference between two values
    MacroRegistry.registerMacro('timeDiff', {
        category: MacroCategory.TIME,
        unnamedArgs: [
            {
                name: 'left',
                sampleValue: '2023-01-01 12:00:00',
                description: 'Left time value.',
                type: 'string',
            },
            {
                name: 'right',
                sampleValue: '2023-01-01 15:00:00',
                description: 'Right time value.',
                type: 'string',
            },
        ],
        description: 'Human-readable difference between two times. Order of times does not matter, it will return the absolute difference.',
        returns: 'Human-readable difference between two times.',
        displayOverride: '{{timeDiff::left::right}}', // Shorten this, otherwise it's too long. Full dates don't really help for understanding the macro.
        exampleUsage: ['{{ timeDiff :: 2023-01-01 12:00:00 :: 2023-01-01 15:00:00 }}'],
        handler: ({ unnamedArgs: [left, right] }) => {
            const diff = moment.duration(moment(left).diff(moment(right)));
            return diff.humanize(true);
        },
    });
}
