// Script to copy the dist directory to the src directory
// This ensures that the bundled library is available to the Tauri application

import { promises as fs } from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

// Get the directory name of the current module
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Define the source and destination directories
const sourceDir = path.join(__dirname, 'dist');
const destDir = path.join(__dirname, 'src', 'dist');

// Function to copy a file
async function copyFile(source, destination) {
    try {
        // Create the destination directory if it doesn't exist
        await fs.mkdir(path.dirname(destination), { recursive: true });
        
        // Copy the file
        await fs.copyFile(source, destination);
        console.log(`Copied ${source} to ${destination}`);
    } catch (error) {
        console.error(`Error copying ${source} to ${destination}:`, error);
    }
}

// Function to copy a directory recursively
async function copyDir(source, destination) {
    try {
        // Create the destination directory if it doesn't exist
        await fs.mkdir(destination, { recursive: true });
        
        // Get all files and directories in the source directory
        const entries = await fs.readdir(source, { withFileTypes: true });
        
        // Copy each file and directory
        for (const entry of entries) {
            const sourcePath = path.join(source, entry.name);
            const destPath = path.join(destination, entry.name);
            
            if (entry.isDirectory()) {
                // Recursively copy the directory
                await copyDir(sourcePath, destPath);
            } else {
                // Copy the file
                await copyFile(sourcePath, destPath);
            }
        }
        
        console.log(`Copied directory ${source} to ${destination}`);
    } catch (error) {
        console.error(`Error copying directory ${source} to ${destination}:`, error);
    }
}

// Main function
async function main() {
    try {
        // Check if the source directory exists
        try {
            await fs.access(sourceDir);
        } catch (error) {
            console.error(`Source directory ${sourceDir} does not exist`);
            process.exit(1);
        }
        
        // Copy the dist directory to the src directory
        await copyDir(sourceDir, destDir);
        
        console.log('Successfully copied dist directory to src directory');
    } catch (error) {
        console.error('Error copying dist directory:', error);
        process.exit(1);
    }
}

// Run the main function
main();
