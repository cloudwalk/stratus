declare cur_downloaded_blocks cursor for
select number, payload
from external_blocks
where number >= $1
order by number;