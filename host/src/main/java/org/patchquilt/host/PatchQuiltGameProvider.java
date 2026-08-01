package org.patchquilt.host;

import java.lang.reflect.InvocationTargetException;
import java.net.URISyntaxException;
import java.nio.file.Path;
import java.util.Collection;
import java.util.List;
import net.fabricmc.api.EnvType;
import org.quiltmc.loader.api.Version;
import org.quiltmc.loader.impl.entrypoint.GameTransformer;
import org.quiltmc.loader.impl.game.EmptyMappingConfiguration;
import org.quiltmc.loader.impl.game.GameProvider;
import org.quiltmc.loader.impl.game.MappingConfiguration;
import org.quiltmc.loader.impl.launch.common.QuiltLauncher;
import org.quiltmc.loader.impl.metadata.qmj.V1ModMetadataBuilder;
import org.quiltmc.loader.impl.util.Arguments;

public final class PatchQuiltGameProvider implements GameProvider {
    private final Arguments arguments = new Arguments();
    private final GameTransformer transformer = new GameTransformer();
    private Path launchDirectory = Path.of(".").toAbsolutePath().normalize();
    private Path hostPath;

    @Override
    public String getGameId() {
        return "minecraft";
    }

    @Override
    public String getGameName() {
        return "Pumpkin";
    }

    @Override
    public String getRawGameVersion() {
        return "26.2";
    }

    @Override
    public String getNormalizedGameVersion() {
        return "26.2";
    }

    @Override
    public Collection<BuiltinMod> getBuiltinMods() {
        V1ModMetadataBuilder metadata = new V1ModMetadataBuilder();
        metadata.id = getGameId();
        metadata.group = "builtin";
        metadata.version = Version.of(getNormalizedGameVersion());
        metadata.name = getGameName();
        return List.of(new BuiltinMod(List.of(hostPath), metadata.build()));
    }

    @Override
    public String getEntrypoint() {
        return PatchQuiltRuntime.class.getName();
    }

    @Override
    public Path getLaunchDirectory() {
        return launchDirectory;
    }

    @Override
    public MappingConfiguration getMappingConfiguration() {
        return new EmptyMappingConfiguration();
    }

    @Override
    public boolean requiresUrlClassLoader() {
        return false;
    }

    @Override
    public boolean isEnabled() {
        return true;
    }

    @Override
    public boolean locateGame(QuiltLauncher launcher, String[] args) {
        arguments.parse(args);
        launchDirectory = Path.of(arguments.getOrDefault("gameDir", ".")).toAbsolutePath().normalize();
        try {
            hostPath = Path.of(PatchQuiltGameProvider.class.getProtectionDomain().getCodeSource().getLocation().toURI());
        } catch (URISyntaxException exception) {
            throw new IllegalStateException(exception);
        }
        System.setProperty("patchquilt.gameDir", launchDirectory.toString());
        return true;
    }

    @Override
    public void initialize(QuiltLauncher launcher) {
        launcher.addToClassPath(hostPath);
        transformer.locateEntrypoints(launcher, List.of(hostPath));
    }

    @Override
    public GameTransformer getEntrypointTransformer() {
        return transformer;
    }

    @Override
    public void unlockClassPath(QuiltLauncher launcher) {
    }

    @Override
    public void launch(ClassLoader loader) {
        Thread.currentThread().setContextClassLoader(loader);
        try {
            Class<?> entrypoint = Class.forName(getEntrypoint(), true, loader);
            entrypoint.getMethod("main", String[].class).invoke(null, (Object) getLaunchArguments(false));
        } catch (ClassNotFoundException | IllegalAccessException | NoSuchMethodException exception) {
            throw new IllegalStateException(exception);
        } catch (InvocationTargetException exception) {
            Throwable cause = exception.getCause();
            if (cause instanceof RuntimeException runtimeException) {
                throw runtimeException;
            }
            throw new IllegalStateException(cause);
        }
    }

    @Override
    public List<Path> getGameJars(String namespace) {
        return List.of(hostPath);
    }

    @Override
    public boolean isGameClass(String name) {
        return name.startsWith("net.minecraft.");
    }

    @Override
    public Arguments getArguments() {
        return arguments;
    }

    @Override
    public String[] getLaunchArguments(boolean sanitize) {
        return arguments.toArray();
    }

    @Override
    public boolean canOpenGui() {
        return false;
    }

    @Override
    public boolean hasAwtSupport() {
        return false;
    }
}
