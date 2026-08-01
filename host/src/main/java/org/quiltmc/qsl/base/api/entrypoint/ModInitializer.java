package org.quiltmc.qsl.base.api.entrypoint;

import org.quiltmc.loader.api.ModContainer;

public interface ModInitializer {
    void onInitialize(ModContainer mod);
}
