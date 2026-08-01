plugins {
    java
}

dependencies {
    compileOnly(project(":host"))
    compileOnly("org.quiltmc:quilt-loader:0.30.0")
    compileOnly("net.fabricmc:sponge-mixin:0.17.2+mixin.0.8.7")
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(25)
    }
}
