# Kitesurf
A Database Client

# Features
* Built in **Rust**. Blazingly fast. Small memory footprint
* PostgreSQL and SQLite
* Tab system
* SQL Editor with syntax highlighting

# Technologies
* Rust
* Dioxus

# Why
I'm always browsing databases, testing features and manipulating data. DB browsers are either too heavy and bloated or too small and lacking features. So I decided to build my own, with all the features I need, while still respecting my RAM.

# Running
Dioxus 0.7 and the `dx` cli are needed.
1. Clone the repository
2. Install the Dioxus desktop dependencies: `sudo apt-get install libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev libgtk-3-dev`
3. Run with `dx serve --desktop -p kitesurfdb`. This will download all dependencies and build the project if you haven't already
