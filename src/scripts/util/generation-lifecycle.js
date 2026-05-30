/**
 * Determines whether an unhandled Generate() error left the foreground
 * generation UI in a locked state that still needs the legacy unblock path.
 *
 * @param {{
 *   dryRun: boolean;
 *   isSendPress: boolean;
 *   isBodyGenerating: boolean;
 *   isGroupGenerating: boolean;
 * }} state
 * @returns {boolean}
 */
export function shouldUnblockGenerationAfterUnhandledError(state) {
    if (state.dryRun || state.isGroupGenerating) {
        return false;
    }

    return Boolean(state.isSendPress || state.isBodyGenerating);
}
