WITH charges AS MATERIALIZED (
    SELECT json_extract(value,'$.key') AS key,
    json_extract(value,'$.meter') AS meter,
    json_extract(value,'$.rule') AS rule,
    json_extract(value,'$.measure.unit') AS unit,
    json_extract(value,'$.evidence') AS evidence,
    json_extract(value,'$.amount') AS amount,
    json_extract(value,'$.limit') AS max_value,
    json_extract(value,'$.window.end') AS reset_at,
    json_extract(value,'$.action') AS action,
    COALESCE(json_extract(value,'$.block_unknown'),0) AS block_unknown,
    json_extract(value,'$.mode') AS mode
    FROM json_each(?1)
), permitted AS MATERIALIZED (
    SELECT NOT EXISTS (SELECT 1 FROM charges c LEFT JOIN resource_usage u ON u.key=c.key WHERE ?2=1 AND ((c.action='block' AND COALESCE(u.consumed,0)+COALESCE((SELECT SUM(reserved) FROM resource_usage WHERE meter=c.meter AND reserved>0),0)+c.amount>c.max_value) OR (c.block_unknown=1 AND (u.evidence='unknown' OR c.evidence='unknown')))) AS ok)
INSERT INTO resource_usage(key,meter,rule,unit,evidence,limit_value,consumed,reserved,reset_at)
SELECT key,meter,rule,unit,evidence,max_value,amount,0,reset_at FROM charges WHERE (SELECT ok FROM permitted)
ON CONFLICT(key) DO UPDATE SET
     rule=excluded.rule,unit=excluded.unit,limit_value=excluded.limit_value,consumed=CASE WHEN (SELECT mode FROM charges WHERE key=excluded.key)='gauge' THEN excluded.consumed ELSE resource_usage.consumed+excluded.consumed END,
     evidence=CASE
         WHEN resource_usage.evidence='unknown' OR excluded.evidence='unknown' THEN 'unknown'
         WHEN resource_usage.evidence='estimated' OR excluded.evidence='estimated' THEN 'estimated'
         WHEN resource_usage.evidence='bounded' OR excluded.evidence='bounded' THEN 'bounded'
         ELSE 'exact'
     END
RETURNING key;
