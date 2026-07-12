# VoxLink ProGuard 混淆规则
# 保留 Tauri 相关类
-keep class com.voxlink.app.** { *; }
-keep class tauri.** { *; }

# 保留音频处理相关
-keep class com.voxlink.app.VoiceInputService { *; }

# 保留 JNI 接口
-keepclasseswithmembernames class * {
    native <methods>;
}

# 通用优化
-optimizations !code/simplification/arithmetic,!field/*,!class/merging/*
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile