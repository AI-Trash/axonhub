import { IconShieldX, IconArrowLeft } from '@tabler/icons-react';
import { useRouter } from '@tanstack/react-router';
import { useEffect } from 'react';

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { useRoutePermissions } from '@/hooks/useRoutePermissions';
import * as m from '@/paraglide/messages';

interface RouteGuardProps {
  children: React.ReactNode;
  requiredScopes?: string[];
  fallbackPath?: string;
  showForbidden?: boolean;
}

export function RouteGuard({ children, requiredScopes = [], fallbackPath = '/', showForbidden = true }: RouteGuardProps) {
  const router = useRouter();
  const { userScopes, isOwner } = useRoutePermissions();

  // 检查用户是否有所需权限
  const hasAccess = isOwner || requiredScopes.length === 0 || requiredScopes.some((scope) => userScopes.includes(scope));

  useEffect(() => {
    if (!hasAccess && !showForbidden) {
      // 如果没有权限且不显示禁止页面，则重定向
      router.navigate({ to: fallbackPath });
    }
  }, [hasAccess, showForbidden, fallbackPath, router]);

  if (!hasAccess) {
    if (showForbidden) {
      return <ForbiddenPage onGoBack={() => router.navigate({ to: fallbackPath })} />;
    }
    return null; // 重定向中，不显示任何内容
  }

  return <>{children}</>;
}

function ForbiddenPage({ onGoBack }: { onGoBack: () => void }) {
  return (
    <div className='flex h-screen items-center justify-center'>
      <div className='max-w-md text-center'>
        <div className='mb-6'>
          <IconShieldX className='mx-auto h-16 w-16 text-red-500' />
        </div>

        <Alert className='mb-6'>
          <IconShieldX className='h-4 w-4' />
          <AlertTitle>{m["common.routeGuard.accessDenied"]()}</AlertTitle>
          <AlertDescription>{m["common.routeGuard.noPermission"]()}</AlertDescription>
        </Alert>

        <Button onClick={onGoBack} variant='outline' className='gap-2'>
          <IconArrowLeft className='h-4 w-4' />
          {m["common.routeGuard.goBack"]()}
        </Button>
      </div>
    </div>
  );
}
