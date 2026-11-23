import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Center } from "@astryxdesign/core/Center";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack } from "@astryxdesign/core/HStack";
import { NavIcon } from "@astryxdesign/core/NavIcon";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { VStack } from "@astryxdesign/core/VStack";
import { Gavel } from "lucide-react";
import { useState } from "react";
import { ApiError, api, setToken } from "../api";
import { go } from "../hash";

export function LoginScreen() {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  return (
    <Center height="100dvh">
      <Card width={400} padding={8} elevation="low">
        <form
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
              <HStack gap={3} vAlign="center"><NavIcon icon={<Gavel size={16} />} /><VStack gap={0}><Heading level={1}>投标管理平台</Heading><Text type="supporting">KnowledgeBrain</Text></VStack></HStack>
              <TextInput id="login-email" data-testid="login-email" label="账号" autoComplete="username" value={email} onChange={setEmail} />
              <TextInput id="login-password" data-testid="login-password" label="密码" autoComplete="current-password" type="password" value={password} onChange={setPassword} />
              {err ? <Banner status="error" title={err} /> : null}
              <Button type="submit" width="100%" label={busy ? "进入中…" : "登录"} variant="primary" isLoading={busy} data-testid="login-submit" />
            </VStack>
          </form>
      </Card>
    </Center>
  );
}
