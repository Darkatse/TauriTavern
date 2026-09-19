import { translate } from '../i18n.js';
import { getCommandErrorMessage } from './command-error-utils.js';

export { extractErrorText } from './command-error-utils.js';

export function toUserFacingErrorText(value) {
    const normalized = getCommandErrorMessage(value);
    return normalized ? translate(normalized) : '';
}
