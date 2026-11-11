-- One durable Team Room task per mission identity in a tenant and room.
-- Fail migration explicitly rather than silently deleting or merging pre-existing tasks.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM team_tasks
        GROUP BY tenant_id, room_id, mission_id
        HAVING COUNT(*) > 1
    ) THEN
        RAISE EXCEPTION 'duplicate team_tasks identities exist for (tenant_id, room_id, mission_id); reconcile them before applying 0007';
    END IF;
END;
$$;

CREATE UNIQUE INDEX team_tasks_tenant_room_mission_uidx
    ON team_tasks (tenant_id, room_id, mission_id);
