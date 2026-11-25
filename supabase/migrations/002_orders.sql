-- Orders: one row per paid USDT transfer found on chain.
-- status: 'pending' (payment found, waiting for owner confirmation) -> 'confirmed' (key may be released)
create table if not exists public.orders (
  tx           text primary key,
  network      text not null,
  payer        text not null,
  key_hash     text not null references public.licenses(key_hash),
  status       text not null default 'pending',
  created_at   timestamptz not null default now(),
  confirmed_at timestamptz
);
alter table public.orders enable row level security;
-- No policies: only the Edge Function (service role) reads and writes orders.

-- Track which license was reserved for which order
alter table public.licenses add column if not exists order_tx text unique;

-- Atomically reserve one free license for a payment. Returns its key_hash, or null if sold out.
create or replace function public.reserve_license(p_tx text)
returns text
language plpgsql
security definer
set search_path = public
as $$
declare
  h text;
begin
  select key_hash into h
  from public.licenses
  where order_tx is null
  order by key_hash
  limit 1
  for update skip locked;

  if h is null then
    return null;
  end if;

  update public.licenses set order_tx = p_tx where key_hash = h;
  return h;
end;
$$;

-- Only the service role may call it (the Edge Function uses the service role)
revoke all on function public.reserve_license(text) from public, anon, authenticated;
grant execute on function public.reserve_license(text) to service_role;

-- To confirm a payment after you checked your wallet, run in SQL Editor:
--   update public.orders set status = 'confirmed', confirmed_at = now() where tx = '0x...';
