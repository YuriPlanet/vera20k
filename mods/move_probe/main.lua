-- move_probe: Step-2 chain probe for the VERA20K Lua host.
--
-- Proves the full gameplay path: a script reads the world (World.GetUnits),
-- resolves a player-owned mobile unit, and issues a real order through the
-- command stream (MoveTo -> Command::Move -> Simulation::issue_local_command),
-- so the simulation itself executes it. Enable by listing "move_probe" in
-- mods/active_mods.txt. Works together with lua_probe.
--
-- A standard skirmish has no mobile units until the player (or a map) fields
-- one, so the probe polls twice a second for up to half an hour and orders
-- the first player-owned unit it sees.
--
-- Deliberately does NOT define the global OnScenarioStart: global callbacks
-- are last-write-wins across mods (the WolfCTOS loader contract), and
-- lua_probe owns that hook in this mod set.

local Probe = {}
local ordered = false

local function order_first_player_unit(frame)
    for _, unit in ipairs(World.GetUnits()) do
        local owner = unit:GetOwner()
        if owner ~= nil and owner:IsHuman() then
            local position = unit:GetPosition()
            if position ~= nil then
                local ok = unit:MoveTo(position.x + 2, position.y)
                ordered = true
                print(string.format(
                    "[move_probe] frame %d: unit %d at (%g, %g) -> MoveTo(%g, %g): %s",
                    frame, unit:GetId(), position.x, position.y,
                    position.x + 2, position.y, tostring(ok)))
                Engine.PrintMessage(string.format(
                    "move_probe: unit %d ordered east", unit:GetId()))
                return
            end
        end
    end
end

function Probe.Update(frame)
    if ordered or frame < 30 then
        return
    end
    -- Poll twice a second; keep the log quiet between progress notes.
    if frame % 60 ~= 0 then
        return
    end
    order_first_player_unit(frame)
    if not ordered and frame % 600 == 0 then
        print("[move_probe] still waiting for a player-owned mobile unit...")
    end
    if not ordered and frame >= 54000 then
        ordered = true
        print("[move_probe] no player-owned mobile unit in 30 minutes; giving up")
        Engine.PrintMessage("move_probe: no player unit found")
    end
end

return Probe
