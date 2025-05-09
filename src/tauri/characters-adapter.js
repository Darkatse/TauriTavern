// Tauri Characters Adapter for SillyTavern
// This module adapts the SillyTavern characters API to use Tauri backend

import * as CharactersAPI from '../scripts/tauri/characters-api.js';
import { isTauri } from '../tauri-bridge.js';

// Original functions to patch
let originalGetCharacters = null;
let originalGetCharacter = null;
let originalCreateCharacter = null;
let originalUpdateCharacter = null;
let originalDeleteCharacter = null;
let originalRenameCharacter = null;
let originalImportCharacter = null;
let originalExportCharacter = null;
let originalUpdateAvatar = null;
let originalGetCharacterChats = null;

/**
 * Initialize the characters adapter
 */
export function initializeCharactersAdapter() {
    if (!isTauri) {
        console.log('Not in Tauri environment, skipping characters adapter initialization');
        return;
    }

    console.log('Initializing Tauri characters adapter');

    try {
        // Get the characters module
        const charactersModule = window.characters;
        if (!charactersModule) {
            console.error('Characters module not found');
            return;
        }

        // Save original functions
        originalGetCharacters = charactersModule.getCharacters;
        originalGetCharacter = charactersModule.getCharacter;
        originalCreateCharacter = charactersModule.createCharacter;
        originalUpdateCharacter = charactersModule.updateCharacter;
        originalDeleteCharacter = charactersModule.deleteCharacter;
        originalRenameCharacter = charactersModule.renameCharacter;
        originalImportCharacter = charactersModule.importCharacter;
        originalExportCharacter = charactersModule.exportCharacter;
        originalUpdateAvatar = charactersModule.updateAvatar;
        originalGetCharacterChats = charactersModule.getCharacterChats;

        // Patch the functions
        charactersModule.getCharacters = async function(shallow = true) {
            try {
                console.log('Using Tauri backend for getCharacters');
                return await CharactersAPI.getAllCharacters(shallow);
            } catch (error) {
                console.error('Error in Tauri getCharacters, falling back to original:', error);
                return originalGetCharacters.apply(this, arguments);
            }
        };

        charactersModule.getCharacter = async function(name) {
            try {
                console.log(`Using Tauri backend for getCharacter: ${name}`);
                return await CharactersAPI.getCharacter(name);
            } catch (error) {
                console.error('Error in Tauri getCharacter, falling back to original:', error);
                return originalGetCharacter.apply(this, arguments);
            }
        };

        charactersModule.createCharacter = async function(characterData) {
            try {
                console.log('Using Tauri backend for createCharacter');
                return await CharactersAPI.createCharacter(characterData);
            } catch (error) {
                console.error('Error in Tauri createCharacter, falling back to original:', error);
                return originalCreateCharacter.apply(this, arguments);
            }
        };

        charactersModule.updateCharacter = async function(name, characterData) {
            try {
                console.log(`Using Tauri backend for updateCharacter: ${name}`);
                return await CharactersAPI.updateCharacter(name, characterData);
            } catch (error) {
                console.error('Error in Tauri updateCharacter, falling back to original:', error);
                return originalUpdateCharacter.apply(this, arguments);
            }
        };

        charactersModule.deleteCharacter = async function(dto) {
            try {
                console.log(`Using Tauri backend for deleteCharacter: ${dto.name}`);
                return await CharactersAPI.deleteCharacter(dto);
            } catch (error) {
                console.error('Error in Tauri deleteCharacter, falling back to original:', error);
                return originalDeleteCharacter.apply(this, arguments);
            }
        };

        charactersModule.renameCharacter = async function(dto) {
            try {
                console.log(`Using Tauri backend for renameCharacter: ${dto.old_name} -> ${dto.new_name}`);
                return await CharactersAPI.renameCharacter(dto);
            } catch (error) {
                console.error('Error in Tauri renameCharacter, falling back to original:', error);
                return originalRenameCharacter.apply(this, arguments);
            }
        };

        charactersModule.importCharacter = async function(dto) {
            try {
                console.log('Using Tauri backend for importCharacter');
                return await CharactersAPI.importCharacter(dto);
            } catch (error) {
                console.error('Error in Tauri importCharacter, falling back to original:', error);
                return originalImportCharacter.apply(this, arguments);
            }
        };

        charactersModule.exportCharacter = async function(dto) {
            try {
                console.log(`Using Tauri backend for exportCharacter: ${dto.name}`);
                return await CharactersAPI.exportCharacter(dto);
            } catch (error) {
                console.error('Error in Tauri exportCharacter, falling back to original:', error);
                return originalExportCharacter.apply(this, arguments);
            }
        };

        charactersModule.updateAvatar = async function(dto) {
            try {
                console.log(`Using Tauri backend for updateAvatar: ${dto.name}`);
                return await CharactersAPI.updateAvatar(dto);
            } catch (error) {
                console.error('Error in Tauri updateAvatar, falling back to original:', error);
                return originalUpdateAvatar.apply(this, arguments);
            }
        };

        charactersModule.getCharacterChats = async function(dto) {
            try {
                console.log(`Using Tauri backend for getCharacterChats: ${dto.name}`);
                return await CharactersAPI.getCharacterChats(dto);
            } catch (error) {
                console.error('Error in Tauri getCharacterChats, falling back to original:', error);
                return originalGetCharacterChats.apply(this, arguments);
            }
        };

        console.log('Tauri characters adapter initialized successfully');
    } catch (error) {
        console.error('Error initializing Tauri characters adapter:', error);
    }
}
