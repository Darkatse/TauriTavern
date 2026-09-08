import { callGenericPopup, POPUP_RESULT, POPUP_TYPE } from '../../../popup.js';
import { translate } from '../../../i18n.js';

async function submitDeviceInput({ title, hint, value = '', placeholder = '', label, pending, submit }) {
    const content = document.createElement('div');
    content.className = 'tt-sync-device-dialog';
    const heading = document.createElement('b');
    heading.textContent = translate(title);
    const description = document.createElement('div');
    description.className = 'tt-sync-muted';
    description.textContent = translate(hint);
    const feedback = document.createElement('div');
    feedback.className = 'tt-sync-device-feedback';
    feedback.setAttribute('role', 'status');
    content.append(heading, description);

    let saved = false;
    await callGenericPopup(content, POPUP_TYPE.INPUT, value, {
        okButton: translate(label),
        cancelButton: translate('Cancel'),
        placeholder,
        rows: 1,
        onOpen(popup) {
            popup.mainInput.setAttribute('aria-label', translate(title));
            popup.mainInput.spellcheck = false;
            popup.mainInput.setAttribute('autocapitalize', 'off');
            popup.mainInput.after(feedback);
        },
        async onClosing(popup) {
            if (popup.result !== POPUP_RESULT.AFFIRMATIVE) return true;
            const input = popup.mainInput.value.trim();
            popup.mainInput.disabled = true;
            popup.buttonControls.inert = true;
            feedback.classList.remove('is-error');
            feedback.textContent = translate(pending);
            try {
                await submit(input);
                saved = true;
                return true;
            } catch (error) {
                const { toUserFacingErrorText } = await import('../../../util/user-facing-error.js');
                feedback.classList.add('is-error');
                feedback.textContent = toUserFacingErrorText(error);
                return false;
            } finally {
                popup.mainInput.disabled = false;
                popup.buttonControls.inert = false;
                if (!saved) popup.mainInput.focus();
            }
        },
    });
    return saved;
}

export function connectLanAddress(client) {
    return submitDeviceInput({
        title: 'Connect manually',
        hint: 'Enter the address shown in Sync on your other device.',
        placeholder: '192.168.1.10:50000',
        label: 'Connect',
        pending: 'Connecting… Confirm on the other device if prompted.',
        submit: address => client.connectLanAddress(address),
    });
}

export function renameLocalDevice(client, name) {
    return submitDeviceInput({
        title: 'Device name',
        hint: 'Other devices will see this name.',
        value: name,
        label: 'Save',
        pending: 'Saving…',
        submit: value => client.setDeviceName(value),
    });
}
