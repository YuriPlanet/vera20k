-- lua_probe: Step-1 chain probe for the VERA20K Lua host.
--
-- Verifies the scenario-start chain end to end: the host fires the global
-- OnScenarioStart hook once per installed world (WolfCTOS contract, re-fired
-- on loaded saves), and Engine.PrintMessage queues a HUD line that the app
-- posts through its message system after the frame commits.
--
-- Enable by listing "lua_probe" in mods/active_mods.txt (the first
-- active_mods.txt the loader finds wins: see src/app/lua/loader.rs).

local Probe = {}

function OnScenarioStart()
    print("[lua_probe] OnScenarioStart fired")
    Engine.PrintMessage("probe online")
end

function Probe.Update(_frame)
    -- quiet: the probe only checks the scenario-start -> HUD chain
end

return Probe
