# Contributing to CILens

Thanks for your interest in contributing! This document provides guidelines to help you get started.

## Getting Started

1. **Read the docs:**
   - [README.md](README.md) - Features, usage, and examples
   - [ARCHITECTURE.md](ARCHITECTURE.md) - Design philosophy and structure

2. **Fork and clone the repository:**

   ```bash
   # Fork the repo on GitHub, then clone your fork
   git clone https://github.com/YOUR-USERNAME/cilens.git
   cd cilens

   # Add upstream remote
   git remote add upstream https://github.com/dsalaza4/cilens.git
   ```

3. **Set up your development environment:**

   ```bash
   # Build the project
   cargo build

   # Run tests
   cargo test

   # Run linting (pedantic clippy)
   cargo lint

   # Format code
   cargo fmt

   # Configure commit message template
   git config commit.template .gitmessage
   ```

4. **Get a GitLab token for testing:**
   - Visit <https://gitlab.com/-/profile/personal_access_tokens>
   - Create a token with `read_api` scope
   - Export it: `export GITLAB_TOKEN="glpat-your-token"`

## Development Workflow

1. **Sync with upstream:**

   ```bash
   git fetch upstream
   git checkout main
   git merge upstream/main
   ```

2. **Create a branch:**

   ```bash
   git checkout -b feat/my-feature
   ```

3. **Make your changes:**
   - Write code
   - Add tests (we prefer unit tests - see ARCHITECTURE.md)
   - Update documentation if needed

4. **Ensure quality:**

   ```bash
   cargo test          # All tests must pass
   cargo lint          # Zero warnings required (pedantic clippy)
   cargo fmt           # Code must be formatted
   ```

5. **Commit your changes:**
   - Follow [Conventional Commits](https://www.conventionalcommits.org/)
   - Format: `type(scope): description`
   - Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `build`, `perf`, `revert`
   - Scopes: `core`, `ci`, `dist`, `gitlab`
   - The commit template (`.gitmessage`) will guide you with the correct format
   - Examples:

   ```text
   feat(gitlab): add support for merge request pipelines
   fix(core): handle corrupted cache files gracefully
   docs(core): update installation instructions
   chore(ci): update github actions workflow
   ```

6. **Push to your fork and create a PR:**

   ```bash
   git push origin feat/my-feature
   ```

   - Open a pull request from your fork to `dsalaza4/cilens:main`
   - CI will run tests and linting automatically
   - Address any feedback from reviewers

## Design Principles

We value **simplicity and pragmatism**. Before adding new features, ask:

1. **Is this simple?** - Avoid over-engineering. Make things as simple as possible while delivering value.
2. **Is this opinionated?** - We prefer good defaults over configurability. Don't add flags unless necessary.
3. **Does this add real value?** - Focus on features that solve actual problems.

See [ARCHITECTURE.md](ARCHITECTURE.md) for full design philosophy.

## Code Style

- **Follow Rust idioms** - Use iterators, `?` operator, `Result`/`Option`
- **Prefer unit tests** - Faster, simpler, more reproducible than integration tests
- **Document public APIs** - All public functions must have rustdoc comments
- **Keep functions focused** - Single responsibility principle
- **No clippy warnings** - We run pedantic clippy (`cargo lint`)

## Testing

- **Run tests:** `cargo test`
- **Run specific test:** `cargo test test_name`
- **Test with output:** `cargo test -- --nocapture`

All tests must pass before merging.

## Questions?

- Open an issue for bugs or feature requests
- Start a discussion for questions or ideas
- Check existing issues before creating new ones

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
