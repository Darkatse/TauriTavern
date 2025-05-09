// Build script for TauriTavern
// This script builds the frontend and starts the Tauri application

import { spawn } from 'child_process';
import { promises as fs } from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

// Get the directory name of the current module
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Create the dist directory if it doesn't exist
async function createDistDirectory() {
    try {
        await fs.mkdir('dist', { recursive: true });
        console.log('Created dist directory');
    } catch (error) {
        console.error('Error creating dist directory:', error);
    }
}

// Run webpack to build the frontend
async function runWebpack() {
    return new Promise((resolve, reject) => {
        console.log('Running webpack...');

        // Use spawn for better output handling
        const webpack = spawn('npx', ['webpack', '--config', 'webpack.config.js'], {
            cwd: __dirname,
            shell: true,
            stdio: 'pipe'
        });

        webpack.stdout.on('data', (data) => {
            console.log(data.toString());
        });

        webpack.stderr.on('data', (data) => {
            console.error(data.toString());
        });

        webpack.on('close', (code) => {
            if (code === 0) {
                console.log('Webpack build completed successfully');
                resolve();
            } else {
                console.error(`Webpack build failed with code ${code}`);
                reject(new Error(`Webpack build failed with code ${code}`));
            }
        });
    });
}

// Start the Tauri application
async function startTauri(dev = true) {
    return new Promise((resolve, reject) => {
        console.log(`Starting Tauri in ${dev ? 'development' : 'production'} mode...`);

        // Use spawn for better output handling
        const tauri = spawn('npx', ['tauri', dev ? 'dev' : 'build'], {
            cwd: __dirname,
            shell: true,
            stdio: 'pipe'
        });

        tauri.stdout.on('data', (data) => {
            console.log(data.toString());
        });

        tauri.stderr.on('data', (data) => {
            console.error(data.toString());
        });

        tauri.on('close', (code) => {
            if (code === 0) {
                console.log('Tauri completed successfully');
                resolve();
            } else {
                console.error(`Tauri failed with code ${code}`);
                reject(new Error(`Tauri failed with code ${code}`));
            }
        });
    });
}

// Function to copy the dist directory to the src directory
async function copyDistToSrc() {
    return new Promise((resolve, reject) => {
        console.log('Copying dist directory to src directory...');

        // Use spawn for better output handling
        const copyDist = spawn('node', ['copy-dist.js'], {
            cwd: __dirname,
            shell: true,
            stdio: 'pipe'
        });

        copyDist.stdout.on('data', (data) => {
            console.log(data.toString());
        });

        copyDist.stderr.on('data', (data) => {
            console.error(data.toString());
        });

        copyDist.on('close', (code) => {
            if (code === 0) {
                console.log('Successfully copied dist directory to src directory');
                resolve();
            } else {
                console.error(`Failed to copy dist directory with code ${code}`);
                reject(new Error(`Failed to copy dist directory with code ${code}`));
            }
        });
    });
}

// Main function
async function main() {
    try {
        // Get command line arguments
        const args = process.argv.slice(2);
        const buildOnly = args.includes('--build-only');
        const prod = args.includes('--prod');

        // Create dist directory
        await createDistDirectory();

        // Run webpack
        await runWebpack();

        // Copy dist directory to src directory
        await copyDistToSrc();

        // Start Tauri if not build-only
        if (!buildOnly) {
            await startTauri(!prod);
        }
    } catch (error) {
        console.error('Build failed:', error);
        process.exit(1);
    }
}

// Run the main function
main();
