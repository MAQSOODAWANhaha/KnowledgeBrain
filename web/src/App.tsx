import { useEffect, useState } from "react";
import { ApiError, api, setToken, token } from "./api";
import { Assets } from "./assets/Assets";
import { ProjectList } from "./bid/ProjectList";
import { Workbench } from "./bid/Workbench";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Center } from "@astryxdesign/core/Center";
import { HStack } from "@astryxdesign/core/HStack";
import { Spinner } from "@astryxdesign/core/Spinner";
import { Text } from "@astryxdesign/core/Text";
import { VStack } from "@astryxdesign/core/VStack";
import { go, parseAssetRoute, parseBidRoute, useHash } from "./hash";
import { LoginScreen } from "./login/LoginScreen";

export function App() {
  const path = useHash();
  const [email, setEmail] = useState<string | null>(null);
  const [verifiedToken, setVerifiedToken] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [sessionError, setSessionError] = useState(false);
  const [sessionRetry, setSessionRetry] = useState(0);

  useEffect(() => {
    let canceled = false;
    if (path === "/login") {
      setReady(true);
      return;
    }
    if (!token()) {
      go("/login");
      setReady(true);
      return;
    }
    const requestToken = token();
    setSessionError(false);
    api
      .me()
      .then((m) => {
        if (canceled || token() !== requestToken) return;
        setEmail(m.email);
        setVerifiedToken(requestToken);
        setReady(true);
      })
      .catch((error: unknown) => {
        if (canceled || token() !== requestToken) return;
        if (error instanceof ApiError && error.status === 401) {
          setToken(null);
          setEmail(null);
          go("/login");
        } else setSessionError(true);
        setReady(true);
      });
    return () => { canceled = true; };
  }, [path, sessionRetry]);

  if (!ready) {
    return <Center height="100dvh"><Spinner label="正在加载" /></Center>;
  }
  if (path === "/login") return <LoginScreen />;
  const retrySession = <Button size="sm" label="重试验证登录" onClick={() => setSessionRetry((value) => value + 1)} />;
  if (email === null || verifiedToken !== token()) return <Center height="100dvh">
    <Card width={420}>
      <VStack gap={4} role={sessionError ? "alert" : "status"}>
        <Text>{sessionError ? "暂时无法验证登录状态，登录信息已保留。" : "正在验证登录状态…"}</Text>
        {sessionError && retrySession}
      </VStack>
    </Card>
  </Center>;
  const asset = parseAssetRoute(path);
  return <>
    {sessionError && <div role="alert" style={{ position: "fixed", insetBlockStart: 56, insetInlineEnd: 16, zIndex: 50 }}>
      <Card elevation="med"><HStack gap={3} vAlign="center"><Text>暂时无法验证登录状态，当前页面和登录信息已保留。</Text>{retrySession}</HStack></Card>
    </div>}
    {asset ? <Assets email={email} route={asset} /> : parseBidRoute(path) ? <Workbench email={email} /> : <ProjectList email={email} />}
  </>;
}
