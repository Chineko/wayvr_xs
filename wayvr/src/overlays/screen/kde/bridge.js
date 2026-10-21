// Temporary, session-scoped KWin script. __UUID__ and __BUS__ are JSON strings.
var targetId = __UUID__;
var bridgeBus = __BUS__;
function belongs(window, root) {
    for (var i = 0; window && i < 16; ++i) {
        if (window === root) return true;
        if (!window.popupWindow) return false;
        window = window.transientFor;
    }
    return false;
}
function processCommand(command) {
    var reply = {seq:command.seq || 0, alive:false, active:false, valid:false, x:0, y:0};
    var windows = workspace.windowList();
    var root = null;
    for (var i = 0; i < windows.length; ++i) {
        if (String(windows[i].internalId) === targetId) root = windows[i];
    }
    if (!root || root.deleted) return reply;
    reply.alive = true;
    if (root.minimized || root.move || root.resize) return reply;
    if (command.activate && !belongs(workspace.activeWindow, root)) {
        workspace.activeWindow = root;
        workspace.raiseWindow(root);
    }
    reply.active = belongs(workspace.activeWindow, root);
    if (!reply.active) return reply;
    var rect = root.frameGeometry;
    var left = rect.x, top = rect.y, right = rect.x + rect.width, bottom = rect.y + rect.height;
    for (var j = 0; j < windows.length; ++j) {
        var w = windows[j];
        if (w !== root && !w.deleted && w.popupWindow && belongs(w, root)) {
            var r = w.frameGeometry;
            left = Math.min(left, r.x); top = Math.min(top, r.y);
            right = Math.max(right, r.x + r.width); bottom = Math.max(bottom, r.y + r.height);
        }
    }
    var width = right - left, height = bottom - top;
    var scale = root.output ? root.output.devicePixelRatio : 0;
    // Refuse input until the displayed capture matches the compositor's bounds.
    if (!(scale > 0) || !(width > 0) || !(height > 0) || !command.extent ||
        Math.abs(command.extent[0] - Math.round(width * scale)) > 2 ||
        Math.abs(command.extent[1] - Math.round(height * scale)) > 2) return reply;
    if (!command.uv || command.uv[0] < 0 || command.uv[0] > 1 ||
        command.uv[1] < 0 || command.uv[1] > 1) return reply;
    reply.x = left + Math.min(command.uv[0] * width, width - 0.01);
    reply.y = top + Math.min(command.uv[1] * height, height - 0.01);
    var at = workspace.windowAt({x:reply.x, y:reply.y}, 1);
    reply.valid = at.length > 0 && belongs(at[0], root);
    return reply;
}
function next(reply) {
    callDBus(bridgeBus, '/org/wayvr/XS', 'org.wayvr.XS.WindowInput', 'Next', JSON.stringify(reply), function(raw) {
        if (typeof raw !== "string") return; // The owner disappeared: stop polling.
        try {
            var command = JSON.parse(raw);
            if (command.stop) return;
            next(processCommand(command));
        } catch (error) {
            callDBus(bridgeBus, '/org/wayvr/XS', 'org.wayvr.XS.WindowInput', 'Next', JSON.stringify({seq:0, alive:false, active:false, valid:false, x:0, y:0}));
        }
    });
}
next({seq:0, alive:true, active:false, valid:false, x:0, y:0});
