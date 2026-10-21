// Enumerate normal app windows across every output; no desktop pointer grab.
var windows = workspace.windowList().filter(function(w) {
    return w.normalWindow && !w.deleted && !w.skipSwitcher;
}).map(function(w) {
    var desktop = String(w.desktopFileName || "");
    var klass = String(w.resourceClass || "");
    var appId = desktop ? "desktop:" + desktop : (klass ? "class:" + klass : "");
    return {app_id:appId, uuid:String(w.internalId), title:String(w.caption), output:w.output ? String(w.output.name) : "Unknown display"};
});
callDBus(__BUS__, '/org/wayvr/XS', 'org.wayvr.XS.WindowInput', 'Choices', JSON.stringify(windows));
