'use strict';

import {
    characterGroupOverlay,
    characters,
    event_types,
    eventSource,
    getCharacters,
    getRequestHeaders,
    buildAvatarList,
    characterToEntity,
    printCharactersDebounced,
    deleteCharacter,
} from '../script.js';

import { favsToHotswap } from './RossAscends-mods.js';
import { loader } from './action-loader.js';
import { convertCharacterToPersona } from './personas.js';
import { callGenericPopup, POPUP_TYPE, Popup } from './popup.js';
import { createTagInput, getTagKeyForEntity, getTagsList, printTagList, tag_map, compareTagsForSort, removeTagFromMap, importTags, tag_import_setting } from './tags.js';
import { t } from './i18n.js';
import { initPopupMenu } from './popup-menu.js';

/**
 * Static object representing the actions of the
 * character context menu override.
 */
class CharacterContextMenu {
    /**
     * Tag one or more characters,
     * opens a popup.
     *
     * @param {Array<number>} selectedCharacters
     */
    static tag = (selectedCharacters) => {
        characterGroupOverlay.bulkTagPopupHandler.show(selectedCharacters);
    };

    /**
     * Duplicate one or more characters
     *
     * @param {number} characterId
     * @returns {Promise<any>}
     */
    static duplicate = async (characterId) => {
        const character = CharacterContextMenu.#getCharacter(characterId);
        const body = { avatar_url: character.avatar };

        const result = await fetch('/api/characters/duplicate', {
            method: 'POST',
            headers: getRequestHeaders(),
            body: JSON.stringify(body),
        });

        if (!result.ok) {
            throw new Error('Character not duplicated');
        }

        const data = await result.json();
        await eventSource.emit(event_types.CHARACTER_DUPLICATED, { oldAvatar: body.avatar_url, newAvatar: data.path });
    };

    /**
     * Favorite a character
     * and highlight it.
     *
     * @param {number} characterId
     * @returns {Promise<void>}
     */
    static favorite = async (characterId) => {
        const character = CharacterContextMenu.#getCharacter(characterId);
        const newFavState = !character.data.extensions.fav;

        const data = {
            name: character.name,
            avatar: character.avatar,
            data: {
                extensions: {
                    fav: newFavState,
                },
            },
            fav: newFavState,
        };

        const mergeResponse = await fetch('/api/characters/merge-attributes', {
            method: 'POST',
            headers: getRequestHeaders(),
            body: JSON.stringify(data),
        });

        if (!mergeResponse.ok) {
            mergeResponse.json().then(json => toastr.error(`Character not saved. Error: ${json.message}. Field: ${json.error}`));
        }

        const element = document.getElementById(`CharID${characterId}`);
        element.classList.toggle('is_fav');
    };

    /**
     * Convert one or more characters to persona,
     * may open a popup for one or more characters.
     *
     * @param {number} characterId
     * @returns {Promise<void>}
     */
    static persona = async (characterId) => void (await convertCharacterToPersona(characterId));

    /**
     * Delete one or more characters,
     * opens a popup.
     *
     * @param {string|string[]} characterKey
     * @param {boolean} [deleteChats]
     * @returns {Promise<void>}
     */
    static delete = async (characterKey, deleteChats = false) => {
        await deleteCharacter(characterKey, { deleteChats: deleteChats });
    };

    static #getCharacter = (characterId) => characters[characterId] ?? null;

    /**
     * Sets up the context menu for the given overlay
     *
     * @param characterGroupOverlay
     */
    constructor(characterGroupOverlay) {
        const contextMenuItems = [
            { id: 'character_context_menu_favorite', callback: characterGroupOverlay.handleContextMenuFavorite },
            { id: 'character_context_menu_duplicate', callback: characterGroupOverlay.handleContextMenuDuplicate },
            { id: 'character_context_menu_delete', callback: characterGroupOverlay.handleContextMenuDelete },
            { id: 'character_context_menu_persona', callback: characterGroupOverlay.handleContextMenuPersona },
            { id: 'character_context_menu_tag', callback: characterGroupOverlay.handleContextMenuTag },
        ];

        contextMenuItems.forEach(contextMenuItem => document.getElementById(contextMenuItem.id).addEventListener('click', () => characterGroupOverlay.runBulkAction(contextMenuItem.callback)));
    }
}

/**
 * Represents a tag control not bound to a single character
 */
class BulkTagPopupHandler {
    /**
     * The characters for this popup
     * @type {number[]}
     */
    characterIds;

    /**
     * A storage of the current mutual tags, as calculated by getMutualTags()
     * @type {object[]}
     */
    currentMutualTags;

    /**
     * Sets up the bulk popup menu handler for the given overlay.
     *
     * Characters can be passed in with the show() call.
     */
    constructor() { }

    /**
     * Append and show the tag control
     *
     * @param {number[]} characterIds - The characters that are shown inside the popup
     */
    show(characterIds) {
        // shallow copy character ids persistently into this tooltip
        this.characterIds = characterIds.slice();

        if (this.characterIds.length == 0) {
            console.log('No characters selected for bulk edit tags.');
            return;
        }

        const content = document.getElementById('bulk_tag_template').content.cloneNode(true).firstElementChild;
        content.querySelector('h3').textContent = t`Modify tags of ${this.characterIds.length} characters`;
        content.querySelector('#bulk_tags_div').dataset.characters = JSON.stringify({ characterIds: this.characterIds });
        const popup = new Popup(content, POPUP_TYPE.TEXT, '', {
            label: content.querySelector('h3'), wider: true, okButton: t`Close`,
        });
        popup.show();

        const entities = this.characterIds.map(id => characterToEntity(characters[id], id)).filter(entity => entity.item !== undefined);
        buildAvatarList($('#bulk_tags_avatars_block'), entities);

        // Print the tag list with all mutuable tags, marking them as removable. That is the initial fill
        printTagList($('#bulkTagList'), { tags: () => this.getMutualTags(), tagOptions: { removable: true } });

        // Tag input with resolvable list for the mutual tags to get redrawn, so that newly added tags get sorted correctly
        createTagInput('#bulkTagInput', '#bulkTagList', { tags: () => this.getMutualTags(), tagOptions: { removable: true } });

        document.querySelector('#bulk_tag_popup_reset').addEventListener('click', this.resetTags.bind(this));
        document.querySelector('#bulk_tag_popup_remove_mutual').addEventListener('click', this.removeMutual.bind(this));
        document.querySelector('#bulk_tag_popup_import_all_tags').addEventListener('click', this.importAllTags.bind(this));
        document.querySelector('#bulk_tag_popup_import_existing_tags').addEventListener('click', this.importExistingTags.bind(this));
    }

    /**
     * Import existing tags for all selected characters
     */
    async importExistingTags() {
        for (const characterId of this.characterIds) {
            await importTags(characters[characterId], { importSetting: tag_import_setting.ONLY_EXISTING });
        }

        $('#bulkTagList').empty();
    }

    /**
     * Import all tags for all selected characters
     */
    async importAllTags() {
        for (const characterId of this.characterIds) {
            await importTags(characters[characterId], { importSetting: tag_import_setting.ALL });
        }

        $('#bulkTagList').empty();
    }

    /**
     * Builds a list of all tags that the provided characters have in common.
     *
     * @returns {Array<object>} A list of mutual tags
     */
    getMutualTags() {
        if (this.characterIds.length == 0) {
            return [];
        }

        if (this.characterIds.length === 1) {
            // Just use tags of the single character
            return getTagsList(getTagKeyForEntity(this.characterIds[0]));
        }

        // Find mutual tags for multiple characters
        const allTags = this.characterIds.map(cid => getTagsList(getTagKeyForEntity(cid)));
        const mutualTags = allTags.reduce((mutual, characterTags) =>
            mutual.filter(tag => characterTags.some(cTag => cTag.id === tag.id)),
        );

        this.currentMutualTags = mutualTags.sort(compareTagsForSort);
        return this.currentMutualTags;
    }

    /**
     * Empty the tag map for the given characters
     */
    resetTags() {
        for (const characterId of this.characterIds) {
            const key = getTagKeyForEntity(characterId);
            if (key) tag_map[key] = [];
        }

        $('#bulkTagList').empty();

        printCharactersDebounced();
    }

    /**
     * Remove the mutual tags for all given characters
     */
    removeMutual() {
        const mutualTags = this.getMutualTags();

        for (const characterId of this.characterIds) {
            for (const tag of mutualTags) {
                removeTagFromMap(tag.id, characterId.toString());
            }
        }

        $('#bulkTagList').empty();

        printCharactersDebounced();
    }
}

class BulkEditOverlayState {
    /**
     *
     * @type {number}
     */
    static browse = 0;

    /**
     *
     * @type {number}
     */
    static select = 1;
}

/**
 * Implement a SingletonPattern, allowing access to the group overlay instance
 * from everywhere via (new CharacterGroupOverlay())
 *
 * @type {Readonly<BulkEditOverlay>}
 */
let bulkEditOverlayInstance = null;

class BulkEditOverlay {
    static containerId = 'rm_print_characters_block';
    static contextMenuId = 'character_context_menu';
    static characterClass = 'character_select';
    static groupClass = 'group_select';
    static bogusFolderClass = 'bogus_folder_select';
    static selectModeClass = 'group_overlay_mode_select';
    static selectedClass = 'character_selected';
    static bulkSelectedCountId = 'bulkSelectedCount';

    static longPressDelay = 2500;

    #state = BulkEditOverlayState.browse;
    #longPress = false;
    #stateChangeCallbacks = [];
    #selectedCharacters = [];
    #bulkTagPopupHandler = new BulkTagPopupHandler();

    /**
     * @typedef {object} LastSelected - An object noting the last selected character and its state.
     * @property {number} [characterId] - The character id of the last selected character.
     * @property {boolean} [select] - The selected state of the last selected character. <c>true</c> if it was selected, <c>false</c> if it was deselected.
     */

    /**
     * @type {LastSelected} - An object noting the last selected character and its state.
     */
    lastSelected = { characterId: undefined, select: undefined };

    #menu;
    #menuPosition = null;
    // Consume the release click of the long press that opened the menu.
    #consumeClick = false;

    /**
     * @type HTMLElement
     */
    container = null;

    get state() {
        return this.#state;
    }

    set state(newState) {
        if (this.#state === newState) return;

        eventSource.emit(event_types.CHARACTER_GROUP_OVERLAY_STATE_CHANGE_BEFORE, newState)
            .then(() => {
                this.#state = newState;
                eventSource.emit(event_types.CHARACTER_GROUP_OVERLAY_STATE_CHANGE_AFTER, this.state);
            });
    }

    get isLongPress() {
        return this.#longPress;
    }

    set isLongPress(longPress) {
        this.#longPress = longPress;
    }

    get stateChangeCallbacks() {
        return this.#stateChangeCallbacks;
    }

    /**
     *
     * @returns {number[]}
     */
    get selectedCharacters() {
        return this.#selectedCharacters;
    }

    /**
     * The instance of the bulk tag popup handler that handles tagging of all selected characters
     *
     * @returns {BulkTagPopupHandler}
     */
    get bulkTagPopupHandler() {
        return this.#bulkTagPopupHandler;
    }

    constructor() {
        if (bulkEditOverlayInstance instanceof BulkEditOverlay)
            return bulkEditOverlayInstance;

        this.container = document.getElementById(BulkEditOverlay.containerId);
        const trigger = document.getElementById('bulkMoreButton');
        const panel = document.getElementById(BulkEditOverlay.contextMenuId);
        this.#menu = initPopupMenu(trigger, panel, {
            onOpen: () => {
                const anchor = trigger.getBoundingClientRect();
                const [x, y] = this.#menuPosition ?? [anchor.left, anchor.bottom];
                this.#menuPosition = null;
                const bounds = panel.getBoundingClientRect();
                panel.style.left = `${Math.max(0, Math.min(x, window.innerWidth - bounds.width))}px`;
                panel.style.top = `${Math.max(0, Math.min(y, window.innerHeight - bounds.height))}px`;
            },
            closesOnClick: () => false,
        });

        eventSource.on(event_types.CHARACTER_GROUP_OVERLAY_STATE_CHANGE_AFTER, this.handleStateChange);
        bulkEditOverlayInstance = Object.freeze(this);
    }

    /**
     * Set the overlay to browse mode
     */
    browseState = () => this.state = BulkEditOverlayState.browse;

    /**
     * Set the overlay to select mode
     */
    selectState = () => this.state = BulkEditOverlayState.select;

    /**
     * Set up a Sortable grid for the loaded page
     */
    onPageLoad = () => {
        this.browseState();

        const elements = this.#getEnabledElements();
        elements.forEach(element => element.addEventListener('touchstart', this.handleHold));
        elements.forEach(element => element.addEventListener('mousedown', this.handleHold));
        elements.forEach(element => element.addEventListener('contextmenu', this.handleDefaultContextMenu));

        elements.forEach(element => element.addEventListener('touchend', this.handleLongPressEnd));
        elements.forEach(element => element.addEventListener('mouseup', this.handleLongPressEnd));
        elements.forEach(element => element.addEventListener('dragend', this.handleLongPressEnd));
        elements.forEach(element => element.addEventListener('touchmove', this.handleLongPressEnd));

    };

    /**
     * Handle state changes
     *
     *
     */
    handleStateChange = () => {
        switch (this.state) {
            case BulkEditOverlayState.browse:
                this.container.classList.remove(BulkEditOverlay.selectModeClass);
                this.#consumeClick = false;
                this.#enableClickEventsForCharacters();
                this.#enableClickEventsForGroups();
                this.selectedCharacters.length = 0;
                this.disableContextMenu();
                this.#disableBulkEditButtonHighlight();
                this.#menu.close();
                break;
            case BulkEditOverlayState.select:
                this.container.classList.add(BulkEditOverlay.selectModeClass);
                this.#disableClickEventsForCharacters();
                this.#disableClickEventsForGroups();
                this.enableContextMenu();
                this.#enableBulkEditButtonHighlight();
                break;
        }

        for (const character of this.#getEnabledElements()) this.#applyCharacterSelection(character);
        this.stateChangeCallbacks.forEach(callback => callback(this.state));
    };

    /** Route pointer and keyboard context-menu requests to the bulk menu. */
    enableContextMenu = () => {
        this.container.addEventListener('contextmenu', this.handleContextMenuShow);
        this.container.addEventListener('keydown', this.handleContextMenuKeydown);
    };

    /**
     * Remove event listeners, allowing the native browser context
     * menu to be opened.
     */
    disableContextMenu = () => {
        this.container.removeEventListener('contextmenu', this.handleContextMenuShow);
        this.container.removeEventListener('keydown', this.handleContextMenuKeydown);
    };

    handleDefaultContextMenu = (event) => {
        if (this.isLongPress) {
            event.preventDefault();
            event.stopPropagation();
            return false;
        }
    };

    /**
     * Opens menu on long-press.
     *
     * @param event - Pointer event
     */
    handleHold = (event) => {
        if (0 !== event.button && event.type !== 'touchstart') return;
        this.#consumeClick = false;
        if (this.#menu.isOpen()) return;

        let cancel = false;

        const cancelHold = (event) => cancel = true;
        this.container.addEventListener('mouseup', cancelHold);
        this.container.addEventListener('touchend', cancelHold);

        this.isLongPress = true;

        setTimeout(() => {
            if (this.isLongPress && !cancel) {
                if (this.state === BulkEditOverlayState.browse) {
                    this.selectState();
                } else if (this.state === BulkEditOverlayState.select) {
                    this.#consumeClick = true;
                    this.#menuPosition = this.#getContextMenuPosition(event);
                    this.#menu.open();
                }
            }

            this.container.removeEventListener('mouseup', cancelHold);
            this.container.removeEventListener('touchend', cancelHold);
        }, BulkEditOverlay.longPressDelay);
    };

    handleLongPressEnd = (event) => {
        this.isLongPress = false;
        if (this.#menu.isOpen()) event.stopPropagation();
    };

    /**
     * Returns the position of the mouse/touch location
     *
     * @param event
     * @returns {(boolean|number|*)[]}
     */
    #getContextMenuPosition = (event) => [
        event.touches?.[0]?.clientX ?? event.clientX,
        event.touches?.[0]?.clientY ?? event.clientY,
    ];

    #stopEventPropagation = (event) => {
        if (this.#menu.isOpen()) this.#menu.close();
        event.stopPropagation();
    };

    #enableClickEventsForGroups = () => this.#getDisabledElements().forEach((element) => element.removeEventListener('click', this.#stopEventPropagation));

    #disableClickEventsForGroups = () => this.#getDisabledElements().forEach((element) => element.addEventListener('click', this.#stopEventPropagation));

    #enableClickEventsForCharacters = () => this.#getEnabledElements().forEach(element => element.removeEventListener('click', this.toggleCharacterSelected));

    #disableClickEventsForCharacters = () => this.#getEnabledElements().forEach(element => {
        element.addEventListener('click', this.toggleCharacterSelected);
    });

    #applyCharacterSelection = (character) => {
        const selected = this.selectedCharacters.includes(Number(character.dataset.chid));
        character.classList.toggle(BulkEditOverlay.selectedClass, selected);
        const primary = character.querySelector(':scope > .sr-only');
        if (this.state === BulkEditOverlayState.select) primary.setAttribute('aria-pressed', String(selected));
        else primary.removeAttribute('aria-pressed');
    };

    #enableBulkEditButtonHighlight = () => $('#bulkEditButton').addClass('bulk_edit_overlay_active').attr('aria-pressed', 'true');

    #disableBulkEditButtonHighlight = () => $('#bulkEditButton').removeClass('bulk_edit_overlay_active').attr('aria-pressed', 'false');

    #getEnabledElements = () => [...this.container.getElementsByClassName(BulkEditOverlay.characterClass)];

    #getDisabledElements = () => [...this.container.getElementsByClassName(BulkEditOverlay.groupClass), ...this.container.getElementsByClassName(BulkEditOverlay.bogusFolderClass)];

    toggleCharacterSelected = event => {
        event.stopPropagation();

        const character = event.currentTarget;

        if (this.#consumeClick) {
            this.#consumeClick = false;
            return;
        }
        if (this.#menu.isOpen()) {
            this.#menu.close();
            return;
        }
        if (event.shiftKey) {
            document.getSelection().removeAllRanges();
            this.handleShiftClick(character);
        } else {
            this.toggleSingleCharacter(character);
        }
    };

    /**
     * When shift click was held down, this function handles the multi select of characters in a single click.
     *
     * If the last clicked character was deselected, and the current one was deselected too, it will deselect all currently selected characters between those two.
     * If the last clicked character was selected, and the current one was selected too, it will select all currently not selected characters between those two.
     * If the states do not match, nothing will happen.
     *
     * @param {HTMLElement} currentCharacter - The html element of the currently toggled character
     */
    handleShiftClick = (currentCharacter) => {
        const characterId = Number(currentCharacter.getAttribute('data-chid'));
        const select = !this.selectedCharacters.includes(characterId);

        if (this.lastSelected.characterId >= 0 && this.lastSelected.select !== undefined) {
            // Only if select state and the last select state match we execute the range select
            if (select === this.lastSelected.select) {
                this.toggleCharactersInRange(currentCharacter, select);
            }
        }
    };

    /**
     * Toggles the selection of a given characters
     *
     * @param {HTMLElement} character - The html element of a character
     * @param {object} param1 - Optional params
     * @param {boolean} [param1.markState] - Whether the toggle of this character should be remembered as the last done toggle
     */
    toggleSingleCharacter = (character, { markState = true } = {}) => {
        const characterId = Number(character.getAttribute('data-chid'));

        const select = !this.selectedCharacters.includes(characterId);
        if (select) {
            this.#selectedCharacters.push(characterId);
        } else {
            this.#selectedCharacters = this.#selectedCharacters.filter(item => characterId !== item);
        }
        this.#applyCharacterSelection(character);
        this.updateSelectedCount();

        if (markState) {
            this.lastSelected.characterId = characterId;
            this.lastSelected.select = select;
        }
    };

    /**
     * Updates the selected count element with the current count
     *
     * @param {number} [countOverride] - optional override for a manual number to set
     */
    updateSelectedCount = (countOverride = undefined) => {
        const count = countOverride ?? this.selectedCharacters.length;
        const status = document.getElementById(BulkEditOverlay.bulkSelectedCountId);
        status.querySelector('[aria-hidden]').textContent = String(count);
        status.querySelector('.sr-only').textContent = t`${count} characters selected`;
    };

    /**
     * Toggles the selection of characters in a given range.
     * The range is provided by the given character and the last selected one remembered in the selection state.
     *
     * @param {HTMLElement} currentCharacter - The html element of the currently toggled character
     * @param {boolean} select - <c>true</c> if the characters in the range are to be selected, <c>false</c> if deselected
     */
    toggleCharactersInRange = (currentCharacter, select) => {
        const currentCharacterId = Number(currentCharacter.getAttribute('data-chid'));
        const characters = Array.from(document.querySelectorAll('#' + BulkEditOverlay.containerId + ' .' + BulkEditOverlay.characterClass));

        const startIndex = characters.findIndex(c => Number(c.getAttribute('data-chid')) === Number(this.lastSelected.characterId));
        const endIndex = characters.findIndex(c => Number(c.getAttribute('data-chid')) === currentCharacterId);

        for (let i = Math.min(startIndex, endIndex); i <= Math.max(startIndex, endIndex); i++) {
            const character = characters[i];
            const characterId = Number(character.getAttribute('data-chid'));
            const isCharacterSelected = this.selectedCharacters.includes(characterId);

            // Only toggle the character if it wasn't on the state we have are toggling towards.
            // Also doing a weird type check, because typescript checker doesn't like the return of 'querySelectorAll'.
            if ((select && !isCharacterSelected || !select && isCharacterSelected) && character instanceof HTMLElement) {
                this.toggleSingleCharacter(character, { markState: currentCharacterId == characterId });
            }
        }
    };

    handleContextMenuKeydown = (event) => {
        if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
            this.handleContextMenuShow(event);
        }
    };

    handleContextMenuShow = (event) => {
        event.preventDefault();
        this.#menuPosition = event.clientX || event.clientY ? this.#getContextMenuPosition(event) : null;
        this.#menu.open();
    };

    runBulkAction = async (action) => {
        document.getElementById('bulkEditButton').focus();
        this.#menu.close();
        try {
            await action();
        } catch (error) {
            console.error('Bulk character action failed', error);
            toastr.error(error.message, t`Bulk edit characters`);
        }
    };

    /**
     * Concurrently handle character favorite requests.
     *
     * @returns {Promise<void>}
     */
    handleContextMenuFavorite = async () => {
        const promises = [];

        for (const characterId of this.selectedCharacters) {
            promises.push(CharacterContextMenu.favorite(characterId));
        }

        await Promise.allSettled(promises);
        await getCharacters();
        await favsToHotswap();
        this.browseState();
    };

    /**
     * Concurrently handle character duplicate requests.
     *
     * @returns {Promise<number>}
     */
    handleContextMenuDuplicate = () => Promise.all(this.selectedCharacters.map(async characterId => CharacterContextMenu.duplicate(characterId)))
        .then(() => getCharacters())
        .then(() => this.browseState());

    /**
     * Sequentially handle all character-to-persona conversions.
     *
     * @returns {Promise<void>}
     */
    handleContextMenuPersona = async () => {
        for (const characterId of this.selectedCharacters) {
            await CharacterContextMenu.persona(characterId);
        }

        this.browseState();
    };

    /**
     * Gets the HTML as a string that is displayed inside the popup for the bulk delete
     *
     * @param {Array<number>} characterIds - The characters that are shown inside the popup
     * @returns String containing the html for the popup content
     */
    static #getDeletePopupContentHtml = (characterIds) => {
        return `
            <h3 class="marginBot5">Delete ${characterIds.length} characters?</h3>
            <span class="bulk_delete_note">
                <i class="fa-solid fa-triangle-exclamation warning margin-r5"></i>
                <b>THIS IS PERMANENT!</b>
            </span>
            <div id="bulk_delete_avatars_block" class="avatars_inline avatars_inline_small tags tags_inline m-t-1"></div>
            <br>
            <div id="bulk_delete_options" class="m-b-1">
                <label for="del_char_checkbox" class="checkbox_label justifyCenter">
                    <input type="checkbox" id="del_char_checkbox" />
                    <span>Also delete the chat files</span>
                </label>
            </div>`;
    };

    /**
     * Request user input before concurrently handle deletion
     * requests.
     *
     * @returns {Promise<number>}
     */
    handleContextMenuDelete = () => {
        const characterIds = this.selectedCharacters;
        const popupContent = $(BulkEditOverlay.#getDeletePopupContentHtml(characterIds));
        const checkbox = popupContent.find('#del_char_checkbox');
        const promise = callGenericPopup(popupContent, POPUP_TYPE.CONFIRM, '', { label: popupContent.filter('h3')[0] })
            .then((accept) => {
                if (!accept) return;

                const deleteChats = checkbox.prop('checked') ?? false;

                const loaderHandle = loader.show({
                    slug: 'bulk-delete',
                    title: t`Bulk Delete`,
                    message: t`Deleting ${characterIds.length} character(s)…`,
                    toastMode: loader.ToastMode.STATIC,
                });
                const avatarList = characterIds.map(id => characters[id]?.avatar).filter(a => a);
                return CharacterContextMenu.delete(avatarList, deleteChats)
                    .then(() => this.browseState())
                    .finally(() => loaderHandle.hide());
            });

        // At this moment the popup is already changed in the dom, but not yet closed/resolved. We build the avatar list here
        const entities = characterIds.map(id => characterToEntity(characters[id], id)).filter(entity => entity.item !== undefined);
        buildAvatarList($('#bulk_delete_avatars_block'), entities);

        return promise;
    };

    /**
     * Attaches and opens the tag menu
     */
    handleContextMenuTag = () => {
        CharacterContextMenu.tag(this.selectedCharacters);
        this.browseState();
    };

    addStateChangeCallback = callback => this.stateChangeCallbacks.push(callback);
}

export { BulkEditOverlayState, CharacterContextMenu, BulkEditOverlay };
