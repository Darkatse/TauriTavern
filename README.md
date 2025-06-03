# TauriTavern

TauriTavern is a rebuild of SillyTavern's backend using Tauri v2 and Rust, while preserving the original frontend. This project aims to provide a more efficient, native, and cross-platform experience for SillyTavern users.

## Features

- **Native Application**: Built with Tauri v2, providing a native application experience on Windows, macOS, and Linux.
- **Efficient Backend**: Rust-based backend for improved performance and resource usage.
- **Clean Architecture**: Modular, low-coupling design with clear separation of concerns.
- **Familiar Frontend**: Preserves the original SillyTavern frontend experience.
- **Detailed Logging**: Comprehensive logging system for easier debugging and troubleshooting.

## Architecture

TauriTavern follows a clean architecture approach with the following layers:

1. **Presentation Layer** (Tauri Commands)
   - Handles communication with the frontend
   - Exposes API endpoints via Tauri commands

2. **Application Layer**
   - Contains use cases and business logic
   - Orchestrates data flow between presentation and domain layers

3. **Domain Layer**
   - Core business entities and logic
   - Domain models and interfaces

4. **Infrastructure Layer**
   - Implements interfaces defined in the domain layer
   - Handles file system operations, data persistence, etc.

## Project Structure

```
src-tauri/
├── src/
│   ├── domain/              # Domain layer
│   │   ├── models/          # Domain entities
│   │   ├── repositories/    # Repository interfaces
│   │   └── errors.rs        # Domain-specific errors
│   ├── application/         # Application layer
│   │   ├── services/        # Application services
│   │   ├── dto/             # Data Transfer Objects
│   │   └── errors.rs        # Application-specific errors
│   ├── infrastructure/      # Infrastructure layer
│   │   ├── repositories/    # Repository implementations
│   │   ├── persistence/     # Data persistence utilities
│   │   └── logging/         # Logging utilities
│   ├── presentation/        # Presentation layer
│   │   ├── commands/        # Tauri commands
│   │   └── errors.rs        # Presentation-specific errors
│   ├── app.rs               # Application setup
│   ├── lib.rs               # Library entry point
│   └── main.rs              # Application entry point
└── Cargo.toml               # Rust dependencies
```

## Development

### Prerequisites

- Rust (latest stable version)
- Node.js (v16 or later)
- Tauri CLI

### Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

### Setup

1. Clone the repository:
   ```
   git clone https://github.com/Darkatse/tauritavern.git
   cd tauritavern
   ```

2. Install dependencies:
   ```
   npm install
   ```

3. Build the frontend:
   ```
   npm run build
   ```

4. Run in development mode:
   ```
   npm run tauri:dev
   ```

### Development Workflow

1. Make changes to the frontend code
2. Run `npm run build` to bundle the libraries
3. Run `npm run tauri:dev` to start the application

### Building

To build the application for production:

```
npm run tauri:build
```

This will create platform-specific installers in the `src-tauri/target/release/bundle` directory.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the same license as SillyTavern - AGPL-3.0.

## Acknowledgements

- [SillyTavern](https://github.com/SillyTavern/SillyTavern) - The original project that this is based on.
- [Tauri](https://tauri.app/) - The framework used to build the native application.
