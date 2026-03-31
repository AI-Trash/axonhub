import { useMutation, useQuery } from '@tanstack/react-query';
import { useRouter } from '@tanstack/react-router';
import { useEffect } from 'react';
import { toast } from 'sonner';

import { graphqlRequest } from '@/gql/graphql';
import { ME_QUERY } from '@/gql/users';
import { authApi } from '@/lib/api-client';
import { useAuthStore, setTokenToStorage, removeTokenFromStorage } from '@/stores/authStore';
import { AuthUser } from '@/stores/authStore';
import * as m from '@/paraglide/messages';
import { getLocale, setLocale } from '@/paraglide/runtime';

export interface SignInInput {
  email: string;
  password: string;
}

interface MeResponse {
  me: AuthUser;
}

export function useMe(enabled = true) {
  const { setUser } = useAuthStore((state) => state.auth);

  const query = useQuery({
    queryKey: ['me'],
    queryFn: async () => {
      const data = await graphqlRequest<MeResponse>(ME_QUERY);
      return data.me;
    },
    enabled: enabled && !!useAuthStore.getState().auth.accessToken,
    retry: false,
  });

  // Update auth store when data changes
  useEffect(() => {
    if (query.data) {
      const userLanguage = query.data.preferLanguage || 'en';

      setUser(query.data);

      // Sync locale with user's preferred language
      if (userLanguage !== getLocale()) {
        setLocale(userLanguage, { reload: false });
      }
    }
  }, [query.data, setUser]);

  return query;
}

export function useSignIn() {
  const { setUser, setAccessToken } = useAuthStore((state) => state.auth);
  const router = useRouter();

  return useMutation({
    mutationFn: async (input: SignInInput) => {
      return await authApi.signIn(input);
    },
    onSuccess: (data) => {
      // Store token in localStorage
      setTokenToStorage(data.token);

      const userLanguage = data.user.preferLanguage || 'en';

      // Update auth store
      setAccessToken(data.token);
      setUser(data.user);

      // Sync locale with user's preferred language
      if (userLanguage !== getLocale()) {
        setLocale(userLanguage, { reload: false });
      }

      toast.success(m["common.success.signedIn"]());

      // Redirect based on user role
      // Owner users go to dashboard, non-owner users go to requests page
      const redirectPath = data.user.isOwner ? '/' : '/project/playground';
      router.navigate({ to: redirectPath });
    },
    onError: (error: any) => {
      const errorMessage = error.message || 'Failed to sign in';
      toast.error(errorMessage);
    },
  });
}

export function useSignOut() {
  const { reset } = useAuthStore((state) => state.auth);
  const router = useRouter();

  return () => {
    // Clear token from localStorage
    removeTokenFromStorage();

    // Clear auth store
    reset();

    toast.success(m["common.success.signedOut"]());

    // Redirect to sign in page
    router.navigate({ to: '/sign-in' });
  };
}
