package org.quiltmc.qsl.base.api.entrypoint.server;

import org.quiltmc.loader.api.ModContainer;

public interface DedicatedServerModInitializer {
    void onInitializeServer(ModContainer mod);
}
