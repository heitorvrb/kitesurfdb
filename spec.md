# DB_CLIENT (codename)

## 1. Overview

My own DB desktop client written in Rust. 

Using Rust with [Insert Desktop framework here]

A DB client for linux (should be cross-platform by using a rust framework). A desktop program where I can save the connection to a database, connect to it, query it through an SQL editor. I could also see a list of the tables, views, etc. in the database, click on it and open in a tab to 'browse' that table. In the tab I can see the query it is doing at all tabs in a small section at the top.

## 2. Planned features

- [ ] Postgres and sqlite, with possibility to add more in the future
- [ ] Should have a tab system with different types of tabs: tables, SQL editors, and others (triggers, etc.)
- [ ] Should be smart about using resources: when a tab is closed, it should free all the tab's resources. 
- [ ] Lightweight and performant
- [ ] Light and Dark modes
