insert into external_balances(address, balance)
values ($1, $2)
on conflict do nothing;