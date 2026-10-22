import { characterGroupOverlay } from '../script.js';
import { BulkEditOverlay, BulkEditOverlayState, CharacterContextMenu } from './BulkEditOverlay.js';
import { event_types, eventSource } from './events.js';

let is_bulk_edit = false;

const enableBulkEdit = () => {
    enableBulkSelect();
    characterGroupOverlay.selectState();
    // show the bulk edit option buttons
    $('.bulkEditOptionElement').show();
    is_bulk_edit = true;
    characterGroupOverlay.updateSelectedCount(0);
};

const disableBulkEdit = () => {
    disableBulkSelect();
    characterGroupOverlay.browseState();
    // hide the bulk edit option buttons
    $('.bulkEditOptionElement').hide();
    is_bulk_edit = false;
    characterGroupOverlay.updateSelectedCount(0);
};

const toggleBulkEditMode = (isBulkEdit) => {
    if (isBulkEdit) {
        disableBulkEdit();
    } else {
        enableBulkEdit();
    }
};

/**
 * Toggles bulk edit mode on/off when the edit button is clicked.
 */
function onEditButtonClick() {
    console.log('Edit button clicked');
    toggleBulkEditMode(is_bulk_edit);
}

/**
 * Toggles the select state of all characters in bulk edit mode to selected. If all are selected, they'll be deselected.
 */
function onSelectAllButtonClick() {
    console.log('Bulk select all button clicked');
    const characters = Array.from(document.querySelectorAll('#' + BulkEditOverlay.containerId + ' .' + BulkEditOverlay.characterClass));
    let atLeastOneSelected = false;
    for (const character of characters) {
        const checked = characterGroupOverlay.selectedCharacters.includes(Number(character.dataset.chid));
        if (!checked && character instanceof HTMLElement) {
            characterGroupOverlay.toggleSingleCharacter(character);
            atLeastOneSelected = true;
        }
    }

    if (!atLeastOneSelected) {
        // If none was selected, trigger click on all to deselect all of them
        for (const character of characters) {
            const checked = characterGroupOverlay.selectedCharacters.includes(Number(character.dataset.chid));
            if (checked && character instanceof HTMLElement) {
                characterGroupOverlay.toggleSingleCharacter(character);
            }
        }
    }
}

/**
 * Deletes all characters that have been selected via the bulk controls.
 */
async function onDeleteButtonClick() {
    console.log('Delete button clicked');

    // We just let the button trigger the context menu delete option
    await characterGroupOverlay.runBulkAction(characterGroupOverlay.handleContextMenuDelete);
}

/** Disable non-character rows while selecting characters. */
function enableBulkSelect() {
    $('#rm_print_characters_block .bogus_folder_select, #rm_print_characters_block .group_select')
        .addClass('disabled').each((_, row) => row.querySelector(':scope > .sr-only').setAttribute('disabled', ''));
    $('#rm_print_characters_block').addClass('bulk_select');
}

function disableBulkSelect() {
    $('#rm_print_characters_block .bogus_folder_select, #rm_print_characters_block .group_select')
        .removeClass('disabled').each((_, row) => row.querySelector(':scope > .sr-only').removeAttribute('disabled'));
    $('#rm_print_characters_block').removeClass('bulk_select');
}

/**
 * Entry point that runs on page load.
 */
export function initBulkEdit() {
    characterGroupOverlay.addStateChangeCallback((state) => {
        if (state === BulkEditOverlayState.select) enableBulkEdit();
        if (state === BulkEditOverlayState.browse) disableBulkEdit();
    });

    $('#bulkEditButton').on('click', onEditButtonClick);
    $('#bulkSelectAllButton').on('click', onSelectAllButtonClick);
    $('#bulkDeleteButton').on('click', onDeleteButtonClick);

    const characterContextMenu = new CharacterContextMenu(characterGroupOverlay);
    eventSource.on(event_types.CHARACTER_PAGE_LOADED, characterGroupOverlay.onPageLoad);
    console.debug('Character context menu initialized', characterContextMenu);
}
