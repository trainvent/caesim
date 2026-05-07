# Legacy Caesim Backend Prototype

This folder keeps the old Rust backend prototype for reference.
The current product path no longer depends on a hosted Rust backend.

## What it does

- stores users in Postgres for development and production
- issues a login verification code
- creates session tokens
- serves `caesim login` and `caesim whoami`
- records assistant usage events
- returns a simple command suggestion for image-library cleanup requests

## Run locally

Only use this if you are experimenting with the legacy proxy layer.

```bash
cd backend
cargo run
```

By default it uses:

- `PORT=3000`
- `DATABASE_URL` from environment, or the Linux Login keychain entry `Ceasim_Supabase`

## Supabase setup

If you want the connection string to stay hidden:

1. Store the **full Supabase Postgres URI** in your Linux Login keychain under the entry name `Ceasim_Supabase`.
2. Make sure the backend can call `secret-tool` on Linux.
3. Start the backend without `DATABASE_URL` set.
4. The backend will read the keychain entry and connect automatically.

Example format:

```bash
postgresql://postgres:[YOUR-PASSWORD]@db.hsqbvdwhgevgbsozythh.supabase.co:5432/postgres
```

Your Supabase project URL is:

```bash
https://hsqbvdwhgevgbsozythh.supabase.co
```

## Important note

This is the prototype layer.
For the real hosted product, the same schema should move to managed Postgres and the login flow should be backed by a hosted auth provider.
