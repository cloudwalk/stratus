select
    number,
    payload
from external_blocks
where number >= $1 and number <= $2
order by number asc;