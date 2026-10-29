WITH charges AS MATERIALIZED (
    SELECT json_extract(value,'$.key') AS key,
           json_extract(value,'$.meter') AS meter,
           json_extract(value,'$.amount') AS amount,
           json_extract(value,'$.limit') AS max_value,
           json_extract(value,'$.action') AS action
    FROM json_each(?1)
)
SELECT NOT EXISTS (
    SELECT 1 FROM charges c LEFT JOIN resource_usage u ON u.key=c.key
    WHERE c.action='block'
      AND COALESCE(u.consumed,0)
          + COALESCE((SELECT SUM(reserved) FROM resource_usage WHERE meter=c.meter AND reserved>0),0)
          + c.amount > c.max_value
) AS allowed;
