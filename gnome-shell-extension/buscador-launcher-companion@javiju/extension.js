import GLib from 'gi://GLib';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const APP_ID = 'com.buscador.launcher';
const UUID = 'buscador-launcher-companion@javiju';
const RETRY_DELAYS_MS = [120, 350, 900];
const TITLE_FALLBACK = 'buscador';

function windowTitle(metaWindow) {
    try {
        return metaWindow.get_title()?.trim() ?? '';
    } catch {
        return '';
    }
}

function gtkApplicationId(metaWindow) {
    try {
        return metaWindow.get_gtk_application_id()?.trim() ?? '';
    } catch {
        return '';
    }
}

function wmClass(metaWindow) {
    try {
        return metaWindow.get_wm_class()?.trim() ?? '';
    } catch {
        return '';
    }
}

function describeWindow(metaWindow) {
    return [
        `title="${windowTitle(metaWindow)}"`,
        `appId="${gtkApplicationId(metaWindow)}"`,
        `wmClass="${wmClass(metaWindow)}"`,
    ].join(' ');
}

function matchesLauncherWindow(metaWindow) {
    const appId = gtkApplicationId(metaWindow).toLowerCase();
    if (appId === APP_ID)
        return true;

    const klass = wmClass(metaWindow).toLowerCase();
    if (klass === APP_ID || klass === 'buscador' || klass.includes('buscador'))
        return true;

    return windowTitle(metaWindow).toLowerCase() === TITLE_FALLBACK;
}

export default class BuscadorLauncherCompanionExtension extends Extension {
    enable() {
        this._hiddenWindows = new Set();
        this._pendingSources = new Map();
        this._trackedWindows = new Set();

        global.display.connectObject(
            'window-created', (_display, metaWindow) => this._trackWindow(metaWindow),
            this);

        for (const actor of global.get_window_actors())
            this._trackWindow(actor.meta_window);

        this._log('enabled');
    }

    disable() {
        global.display.disconnectObject(this);

        for (const metaWindow of this._trackedWindows)
            metaWindow.disconnectObject?.(this);
        this._trackedWindows.clear();

        for (const sourceIds of this._pendingSources.values()) {
            for (const sourceId of sourceIds)
                GLib.source_remove(sourceId);
        }
        this._pendingSources.clear();

        for (const metaWindow of this._hiddenWindows) {
            try {
                metaWindow.show_in_window_list?.();
            } catch (error) {
                this._log(`failed to restore ${describeWindow(metaWindow)}: ${error}`);
            }
        }
        this._hiddenWindows.clear();

        this._log('disabled');
    }

    _trackWindow(metaWindow) {
        if (!metaWindow || this._trackedWindows.has(metaWindow))
            return;

        this._trackedWindows.add(metaWindow);
        metaWindow.connectObject('unmanaged', () => this._untrackWindow(metaWindow), this);

        this._scheduleHideAttempts(metaWindow);
        this._maybeHideWindow(metaWindow, 'window-created');
    }

    _untrackWindow(metaWindow) {
        metaWindow.disconnectObject?.(this);
        this._trackedWindows.delete(metaWindow);
        this._hiddenWindows.delete(metaWindow);
        this._clearPendingSources(metaWindow);
    }

    _scheduleHideAttempts(metaWindow) {
        const sourceIds = new Set();
        this._pendingSources.set(metaWindow, sourceIds);

        for (const delay of RETRY_DELAYS_MS) {
            let sourceId = 0;
            sourceId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, delay, () => {
                sourceIds.delete(sourceId);
                this._maybeHideWindow(metaWindow, `retry-${delay}ms`);
                if (sourceIds.size === 0)
                    this._pendingSources.delete(metaWindow);
                return GLib.SOURCE_REMOVE;
            });
            sourceIds.add(sourceId);
        }
    }

    _maybeHideWindow(metaWindow, reason) {
        if (!matchesLauncherWindow(metaWindow))
            return false;

        if (this._hiddenWindows.has(metaWindow))
            return true;

        if (typeof metaWindow.hide_from_window_list !== 'function') {
            this._log(`hide_from_window_list() is unavailable for ${describeWindow(metaWindow)}`);
            return false;
        }

        try {
            metaWindow.hide_from_window_list();
            this._clearPendingSources(metaWindow);
            this._hiddenWindows.add(metaWindow);
            this._log(`hid ${describeWindow(metaWindow)} via ${reason}`);
            return true;
        } catch (error) {
            this._log(`failed to hide ${describeWindow(metaWindow)}: ${error}`);
            return false;
        }
    }

    _clearPendingSources(metaWindow) {
        const sourceIds = this._pendingSources.get(metaWindow);
        if (!sourceIds)
            return;

        for (const sourceId of sourceIds)
            GLib.source_remove(sourceId);
        this._pendingSources.delete(metaWindow);
    }

    _log(message) {
        console.log(`[${UUID}] ${message}`);
    }
}
