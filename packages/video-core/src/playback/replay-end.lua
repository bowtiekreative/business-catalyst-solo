local mp = require 'mp'
local replaying = false
mp.observe_property('eof-reached', 'bool', function(_, eof)
    if eof then mp.set_property_bool('pause', true) else replaying = false end
end)
mp.observe_property('pause', 'bool', function(_, paused)
    if not paused and not replaying and mp.get_property_bool('eof-reached', false) then
        replaying = true
        mp.commandv('seek', '0', 'absolute+exact')
        mp.add_timeout(0, function() mp.set_property_bool('pause', false) end)
    end
end)
