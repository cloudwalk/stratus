select
    payload
from
    external_blocks
where number >= $1 and number <= $2;