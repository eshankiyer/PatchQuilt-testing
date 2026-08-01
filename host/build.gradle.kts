plugins {
    application
}

dependencies {
    implementation("org.quiltmc:quilt-loader:0.30.0")
    implementation("net.fabricmc:sponge-mixin:0.17.2+mixin.0.8.7") {
        exclude(group = "net.minecraft", module = "launchwrapper")
    }
    implementation("org.quiltmc:quilt-json5:1.0.4+final")
    implementation("org.quiltmc:quilt-config:1.3.3")
    implementation("org.ow2.asm:asm:9.9")
    implementation("org.ow2.asm:asm-analysis:9.9")
    implementation("org.ow2.asm:asm-commons:9.9")
    implementation("org.ow2.asm:asm-tree:9.9")
    implementation("org.ow2.asm:asm-util:9.9")
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(25)
    }
}

application {
    mainClass = "org.patchquilt.host.PatchQuiltLauncher"
}

tasks.test {
    useJUnitPlatform()
}
