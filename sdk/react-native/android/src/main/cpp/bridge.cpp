#include <jni.h>
#include <string>

extern "C" char *swarmie_client_execute(const char *command);
extern "C" void swarmie_client_free(char *result);

extern "C" JNIEXPORT jstring JNICALL
Java_app_swarmie_client_SwarmieClientModule_invoke(JNIEnv *env, jobject, jstring command) {
  // JNI modified UTF-8 differs from UTF-8 for emoji. Marshal through byte arrays.
  auto stringClass = env->FindClass("java/lang/String");
  auto getBytes = env->GetMethodID(stringClass, "getBytes", "(Ljava/lang/String;)[B");
  auto charset = env->NewStringUTF("UTF-8");
  auto bytes = static_cast<jbyteArray>(env->CallObjectMethod(command, getBytes, charset));
  if (env->ExceptionCheck()) return nullptr;
  std::string input(env->GetArrayLength(bytes), '\0');
  env->GetByteArrayRegion(bytes, 0, input.size(), reinterpret_cast<jbyte *>(input.data()));
  if (env->ExceptionCheck()) return nullptr;
  auto output = swarmie_client_execute(input.c_str());
  if (!output) return nullptr;
  std::string result(output);
  swarmie_client_free(output);
  auto resultBytes = env->NewByteArray(result.size());
  env->SetByteArrayRegion(resultBytes, 0, result.size(), reinterpret_cast<const jbyte *>(result.data()));
  auto constructor = env->GetMethodID(stringClass, "<init>", "([BLjava/lang/String;)V");
  return static_cast<jstring>(env->NewObject(stringClass, constructor, resultBytes, charset));
}
