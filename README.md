# Rust Discord Bot

A Discord bot built with Rust following Clean Architecture principles. This project serves as a comprehensive tutorial for learning Rust and software architecture.

## 🏗️ Architecture

This bot follows **Clean Architecture** (Hexagonal Architecture) with three distinct layers:

```
┌─────────────────────────────────────────┐
│          Discord Layer                  │  (Thin adapter)
│  Commands, Events, Voice Connections    │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│          Core Layer                     │  (Pure domain logic)
│  Services, Models, Business Rules       │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│          Infra Layer                    │  (External implementations)
│  API Clients, Database, File System     │
└─────────────────────────────────────────┘
```

### Layers

- **`core/`** - Pure business logic, platform-agnostic
- **`infra/`** - Implementations of core traits (databases, APIs)
- **`discord/`** - Discord-specific adapters (commands, events)

See [AGENTS.md](AGENTS.md) for detailed architecture documentation.

## ✨ Features

### Currently Implemented

- ✅ **Leveling System** - Users earn XP by chatting and level up
  - `/level`, `/profile` - Check your level and XP
  - `/leaderboard` - View server leaderboard
  - `/achievements` - View earned achievements
  - `/daily` - Claim daily XP rewards
- ✅ **GitHub Integration** - Track repository updates
  - `/github` - Manage subscriptions to repositories
  - Background polling for commits and issues
- ✅ **Server Logging** - Comprehensive event logging
  - `/logging` - Configure logging channels
  - Tracks messages, member changes, voice state, and more
- ✅ **AI Assistant** - Intelligent responses via OpenRouter
  - Responds to mentions with context-aware answers
  - Configurable persona and reasoning capabilities
- ✅ **Server Stats** - Track server growth
  - `/serverstats` - View server statistics
  - Automatic channel counters for member count
- ✅ **Timezones** - User timezone management
  - `/timezones` - Set and view user timezones

### Coming Soon

- 🎵 Music Playing (Spotify, YouTube)
- 💻 Interactive Code Execution & Challenges

## 🚀 Getting Started

### Prerequisites

- Rust (latest stable) - [Install Rust](https://rustup.rs/)
- A Discord Bot Token - [Create a bot](https://discord.com/developers/applications)

### Setup

1. **Clone the repository**
   ```bash
   git clone <your-repo-url>
   cd rustDiscordBot
   ```

2. **Create a `.env` file**
   ```bash
   cp .env.example .env
   ```
   
   Then edit `.env` and add your tokens:
   ```env
   DISCORD_TOKEN=your_discord_token
   GITHUB_TOKEN=your_github_token
   OPENROUTER_API_KEY=your_openrouter_key
   ```

3. **Build and run**
   ```bash
   cargo run
   ```

### Inviting the Bot

1. Go to the [Discord Developer Portal](https://discord.com/developers/applications)
2. Select your application
3. Go to OAuth2 → URL Generator
4. Select scopes: `bot`, `applications.commands`
5. Select permissions: 
   - Send Messages
   - Read Message History
   - Use Slash Commands
6. Copy the generated URL and open it in your browser
7. Select a server and authorize

## 📚 Learning Path

This project is designed as a comprehensive Rust tutorial. Each module is heavily commented to explain:

- **Rust concepts** - Ownership, borrowing, traits, async/await
- **Architecture patterns** - Dependency injection, ports & adapters
- **Best practices** - Error handling, testing, documentation

### Recommended Reading Order

1. `core/leveling/mod.rs` - Start here to understand core business logic
2. `infra/leveling/in_memory.rs` - See how traits are implemented
3. `discord/commands/leveling.rs` - Learn how Discord commands work
4. `src/main.rs` - Understand dependency injection and bot initialization

## 🧪 Testing

Run the test suite:
```bash
cargo test
```

Tests are included in each module demonstrating:
- Unit testing pure business logic
- Testing trait implementations
- Async testing with `tokio::test`

## 🛠️ Development

### Project Structure

```
src/
├── main.rs                 # Entry point & dependency injection
├── core/                   # Business logic (platform-agnostic)
│   ├── leveling/          # Leveling system domain
│   ├── ai/                # AI service domain
│   ├── github/            # GitHub tracking domain
│   ├── logging/           # Logging domain
│   └── mod.rs
├── infra/                  # External implementations
│   ├── leveling/          # XP storage implementations
│   ├── ai/                # OpenRouter client
│   ├── github/            # GitHub API client
│   └── mod.rs
└── discord/                # Discord adapters
    ├── commands/          # Slash commands
    └── mod.rs
```

### Adding a New Feature

Follow the architecture guide:

1. **Core** - Define domain models, business rules, and trait interfaces
2. **Infra** - Implement the traits for external systems
3. **Discord** - Create thin command/event handlers that call core services

## 📖 Documentation

- Inline code comments explain every design decision
- Run `cargo doc --open` for API documentation

## 🤝 Contributing

This is a learning project! Contributions are welcome, especially:

- Additional tutorial comments
- More example features
- Documentation improvements
- Bug fixes

## 📝 License

MIT

## 🙏 Acknowledgments

Built with:
- [poise](https://github.com/serenity-rs/poise) - Command framework
- [serenity](https://github.com/serenity-rs/serenity) - Discord library
- [tokio](https://tokio.rs/) - Async runtime
