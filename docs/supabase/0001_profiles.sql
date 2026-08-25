-- Vivianne accounts — run this once in the Supabase SQL editor
-- (Project → SQL Editor → New query → paste → Run) for project
-- qlanrjfdgqeqxnznebnq. Safe to re-run: every statement is idempotent.

create table if not exists public.profiles (
  id uuid primary key references auth.users (id) on delete cascade,
  display_name text,
  avatar_url text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

alter table public.profiles enable row level security;

drop policy if exists "profiles are viewable by the owner" on public.profiles;
create policy "profiles are viewable by the owner"
  on public.profiles for select
  using (auth.uid() = id);

drop policy if exists "profiles are updatable by the owner" on public.profiles;
create policy "profiles are updatable by the owner"
  on public.profiles for update
  using (auth.uid() = id);

drop policy if exists "profiles are insertable by the owner" on public.profiles;
create policy "profiles are insertable by the owner"
  on public.profiles for insert
  with check (auth.uid() = id);

-- Auto-create a profile row the moment someone signs up, seeded from the
-- part of their email before the @ — so `display_name` is never null for a
-- new account, and the app never has to special-case "profile row missing."
create or replace function public.handle_new_user()
returns trigger
language plpgsql
security definer set search_path = public
as $$
begin
  insert into public.profiles (id, display_name)
  values (new.id, split_part(new.email, '@', 1))
  on conflict (id) do nothing;
  return new;
end;
$$;

drop trigger if exists on_auth_user_created on auth.users;
create trigger on_auth_user_created
  after insert on auth.users
  for each row execute procedure public.handle_new_user();

-- Keep `updated_at` honest on every edit.
create or replace function public.set_updated_at()
returns trigger
language plpgsql
as $$
begin
  new.updated_at = now();
  return new;
end;
$$;

drop trigger if exists on_profiles_updated on public.profiles;
create trigger on_profiles_updated
  before update on public.profiles
  for each row execute procedure public.set_updated_at();
