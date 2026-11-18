import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Heading } from "@astryxdesign/core/Heading";
import { TextInput } from "@astryxdesign/core/TextInput";
import { VStack } from "@astryxdesign/core/VStack";
import { useState } from "react";
import { ApiError, api, setToken } from "../api";
import { go } from "../hash";
import "./login.css";

export function LoginScreen() {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  return (
    <main className="grid min-h-dvh lg:grid-cols-[minmax(0,1.15fr)_minmax(420px,0.85fr)]">
      <section className="flex flex-col justify-center gap-6 bg-sky px-6 py-16 text-white sm:px-10 lg:px-16">
        <span className="block h-1 w-12 bg-wait" />
        <Heading level={1} type="display-2" color="inherit">KnowledgeBrain</Heading>
      </section>
      <section className="flex items-center bg-sky-wash px-6 py-12 sm:px-12 lg:px-16">
        <form
          className="w-full max-w-md"
          data-testid="login-form"
          onSubmit={async (event) => {
            event.preventDefault();
            if (!email.trim() || !password) {
              setErr("请填写账号和密码");
              return;
            }
            setErr("");
            setBusy(true);
            try {
              const result = await api.login(email.trim(), password);
              setToken(result.token);
              go("/");
            } catch (ex) {
              setErr(ex instanceof ApiError ? "登录失败，请再试一次" : ex instanceof Error ? ex.message : "网络错误");
            } finally {
              setBusy(false);
            }
          }}
        >
          <VStack gap={5}>
            <Heading level={2} type="display-3">登录</Heading>
            <TextInput id="login-email" data-testid="login-email" label="账号" autoComplete="username" value={email} onChange={setEmail} containerClassName="login-field login-line" />
            <TextInput id="login-password" data-testid="login-password" label="密码" autoComplete="current-password" type="password" value={password} onChange={setPassword} containerClassName="login-field login-line login-field-late" />
            {err ? <Banner status="error" title={err} /> : null}
            <div className="login-field-last">
              <Button type="submit" width="100%" label={busy ? "进入中…" : "进入"} variant="primary" isLoading={busy} data-testid="login-submit" />
            </div>
          </VStack>
        </form>
      </section>
    </main>
  );
}
