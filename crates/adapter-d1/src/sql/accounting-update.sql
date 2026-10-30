UPDATE resource_usage
SET consumed=consumed+json_extract(?1,'$.amount'),
    evidence=CASE
        WHEN resource_usage.evidence='unknown' OR json_extract(?1,'$.evidence')='unknown' THEN 'unknown'
        WHEN resource_usage.evidence='estimated' OR json_extract(?1,'$.evidence')='estimated' THEN 'estimated'
        WHEN resource_usage.evidence='bounded' OR json_extract(?1,'$.evidence')='bounded' THEN 'bounded'
        ELSE 'exact'
    END
WHERE key=json_extract(?1,'$.key');
