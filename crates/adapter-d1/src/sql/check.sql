WITH charges AS MATERIALIZED (
    SELECT json_extract(value,'$.key') AS key,
           json_extract(value,'$.meter') AS meter,
           json_extract(value,'$.amount') AS amount,
           json_extract(value,'$.limit') AS max_value,
           json_extract(value,'$.action') AS action,
           json_extract(value,'$.evidence') AS evidence,
           COALESCE(json_extract(value,'$.block_unknown'),0) AS block_unknown
    FROM json_each(?1)
)
SELECT NOT EXISTS (
    SELECT 1 FROM charges c LEFT JOIN resource_usage u ON u.key=c.key
    WHERE (c.action='block'
      AND COALESCE(u.consumed,0)
          + COALESCE((SELECT SUM(reserved) FROM resource_usage WHERE meter=c.meter AND reserved>0),0)
          + c.amount > c.max_value)
       OR (c.block_unknown=1 AND (u.evidence='unknown' OR c.evidence='unknown'))
) AS allowed;
