INSERT INTO resource_usage(key,meter,rule,unit,evidence,limit_value,consumed,reserved,reset_at)
VALUES (
    json_extract(?1,'$.key'),json_extract(?1,'$.meter'),json_extract(?1,'$.rule'),
    json_extract(?1,'$.measure.unit'),json_extract(?1,'$.evidence'),
    json_extract(?1,'$.limit'),json_extract(?1,'$.amount'),0,json_extract(?1,'$.window.end')
)
ON CONFLICT(key) DO UPDATE SET
    rule=excluded.rule,unit=excluded.unit,limit_value=excluded.limit_value,
    consumed=resource_usage.consumed+excluded.consumed,
    evidence=CASE
        WHEN resource_usage.evidence='unknown' OR excluded.evidence='unknown' THEN 'unknown'
        WHEN resource_usage.evidence='estimated' OR excluded.evidence='estimated' THEN 'estimated'
        WHEN resource_usage.evidence='bounded' OR excluded.evidence='bounded' THEN 'bounded'
        ELSE 'exact'
    END;
